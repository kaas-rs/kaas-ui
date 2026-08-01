//! The response envelope, and the single place a `PerItem` is split.
//!
//! Partial results are the default rather than a special case. Describing 50
//! topics of which 2 do not exist is `200 OK` with 48 items and 2 errors — the
//! call succeeded, some resources did not — and collapsing that to a 500 would
//! discard the property on precisely the clusters that most need a UI.
//!
//! No handler builds an envelope by hand. [`Envelope::from_per_item`] is the
//! only split, so there is one place for it to be wrong.

use kafka_admin::types::PerItem;
use kafka_conn::Error;
use serde::Serialize;
use utoipa::ToSchema;

use crate::error::{ErrorKind, UnsupportedApiDetail};

/// What every data endpoint returns.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Envelope<T> {
    /// The resources that answered.
    pub items: Vec<T>,
    /// The resources that did not. Non-empty is still `200 OK`.
    pub errors: Vec<ResourceError>,
    /// Age of the metadata snapshot the answer was built from, where one was
    /// involved. This is what makes "as of 4 seconds ago" honest.
    pub snapshot_age_ms: Option<u64>,
    /// How many resources matched before paging or truncation.
    ///
    /// Present only where the endpoint pages. Filtering and sorting happen on
    /// the server — a fleet with a five-thousand-topic cluster in it is real —
    /// so the browser needs to be told what it is looking at a window into.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<usize>,
}

/// One resource's failure, named.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResourceError {
    /// Which resource — a topic name, a broker id, a group id.
    pub resource: String,
    /// The taxonomy.
    pub kind: ErrorKind,
    /// The broker's error code name, when this build has one for it.
    pub code: Option<String>,
    /// The error code's number.
    ///
    /// Present even when `code` is null: against a broker newer than the codec
    /// `ErrorCode::Unknown(i16)` is all there is, and the number is the only
    /// searchable thing.
    pub code_number: Option<i16>,
    /// The error, rendered.
    pub message: String,
    /// Both version ranges, when the failure was an unsupported api.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unsupported_api: Option<UnsupportedApiDetail>,
    /// Whether retrying this one resource is worth offering.
    pub retriable: bool,
}

impl ResourceError {
    /// Build from a kaas-lib error against a named resource.
    pub fn new(resource: impl Into<String>, error: &Error) -> Self {
        let code = error.code();
        Self {
            resource: resource.into(),
            kind: ErrorKind::of(error),
            code: code.and_then(|c| c.name()).map(str::to_owned),
            code_number: code.map(|c| c.code()),
            message: error.to_string(),
            unsupported_api: UnsupportedApiDetail::of(error),
            retriable: error.retriable(),
        }
    }
}

impl<T> Envelope<T> {
    /// An envelope with no failures.
    pub fn new(items: Vec<T>) -> Self {
        Self {
            items,
            errors: Vec::new(),
            snapshot_age_ms: None,
            total: None,
        }
    }

    /// A single-item envelope, for the detail endpoints.
    pub fn one(item: T) -> Self {
        Self::new(vec![item])
    }

    /// Attach the age of the snapshot the answer came from.
    pub fn with_snapshot_age(mut self, age: std::time::Duration) -> Self {
        // `u64::try_from` rather than `as`: a duration wider than u64
        // milliseconds is not something to silently wrap.
        self.snapshot_age_ms = u64::try_from(age.as_millis()).ok();
        self
    }

    /// Record how many resources matched before paging.
    pub fn with_total(mut self, total: usize) -> Self {
        self.total = Some(total);
        self
    }

    /// Attach failures that were not per-item — an enrichment call that did
    /// not answer, say.
    pub fn with_errors(mut self, errors: impl IntoIterator<Item = ResourceError>) -> Self {
        self.errors.extend(errors);
        self
    }

    /// Split a kaas-lib `PerItem` into items and errors.
    ///
    /// `name` renders the key for the error side; `convert` builds the DTO for
    /// the success side. This is the only split in the codebase.
    pub fn from_per_item<K, U>(
        items: PerItem<K, U>,
        name: impl Fn(&K) -> String,
        convert: impl Fn(&K, U) -> T,
    ) -> Self {
        let mut oks = Vec::new();
        let mut errors = Vec::new();
        for (key, outcome) in items {
            match outcome {
                Ok(value) => oks.push(convert(&key, value)),
                Err(error) => errors.push(ResourceError::new(name(&key), &error)),
            }
        }
        Self {
            items: oks,
            errors,
            snapshot_age_ms: None,
            total: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kafka_conn::ErrorCode;

    #[test]
    fn partial_failure_is_a_result_not_an_error() {
        let per_item: PerItem<String, i32> = vec![
            ("orders".to_owned(), Ok(6)),
            (
                "shipments".to_owned(),
                Err(Error::from_code(ErrorCode::UnknownTopicOrPartition, None)),
            ),
            ("payments".to_owned(), Ok(3)),
        ];

        let envelope: Envelope<i32> = Envelope::from_per_item(per_item, |k| k.clone(), |_, v| v);

        assert_eq!(envelope.items, vec![6, 3]);
        assert_eq!(envelope.errors.len(), 1);
        assert_eq!(envelope.errors[0].resource, "shipments");
        assert_eq!(
            envelope.errors[0].code.as_deref(),
            Some("UNKNOWN_TOPIC_OR_PARTITION")
        );
        assert_eq!(envelope.errors[0].code_number, Some(3));
    }

    #[test]
    fn an_unknown_code_still_carries_its_number() {
        // The broker is newer than the codec. The name is gone; the number is
        // the only thing anyone can search for, so it must survive.
        let error = Error::Broker {
            code: ErrorCode::Unknown(30000),
            message: None,
        };
        let rendered = ResourceError::new("orders", &error);
        assert_eq!(rendered.code, None);
        assert_eq!(rendered.code_number, Some(30000));
    }

    #[test]
    fn the_envelope_serialises_camel_case() {
        let envelope =
            Envelope::<i32>::new(vec![1]).with_snapshot_age(std::time::Duration::from_millis(4213));
        let json = serde_json::to_value(&envelope).unwrap();
        assert_eq!(json["snapshotAgeMs"], 4213);
        assert!(json["errors"].as_array().unwrap().is_empty());
    }
}
