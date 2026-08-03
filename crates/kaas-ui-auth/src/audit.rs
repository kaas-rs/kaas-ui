//! Who read which payloads.
//!
//! In a tool that cannot write, the audit question is **reads**: who opened
//! which topic's messages, on which cluster, when, and how much of it they
//! saw. Nothing is being changed, which is exactly why this is the log that
//! matters and exactly why it is the one most likely to be skipped.
//!
//! # Two rules that make it an audit log rather than a log
//!
//! **Written before the payload is disclosed.** Not after, not alongside. The
//! record exists before the bytes leave the process, so there is no ordering in
//! which somebody reads a topic and no entry appears.
//!
//! **A failed write fails the request.** [`Audit::record`] returns a `Result`
//! and every caller propagates it — a read that could not be recorded does not
//! happen. An audit log that is best-effort is not an audit log; it is a log.
//!
//! # Metadata is not audited
//!
//! Listing topics is not reading a payload. Auditing every request would bury
//! the entries that matter under a fleet view's polling, and the boundary is
//! the same one the `messages` grant draws: this records what that grant
//! permits.
//!
//! # The sink
//!
//! One line of JSON per read, to stdout, where this cluster's observability
//! stack already collects it. The phase file offers SQLite via `sqlx` as an
//! alternative and it is **not built**: a database is a second thing to run,
//! back up and migrate, for a log nobody has yet asked to query. The writer is
//! injectable, so adding one later changes this module and nothing else.

use std::io::Write;
use std::sync::Mutex;
use std::time::SystemTime;

use serde::Serialize;

/// Which read it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// The newest records of a topic, one shot.
    Tail,
    /// One bounded page of a window.
    Page,
    /// One record, whole, by partition and offset.
    Record,
    /// A stream opened. Rows are not counted: the entry is written when the
    /// stream starts, because that is when disclosure begins.
    Stream,
}

/// One disclosure.
///
/// The fields the phase file names — timestamp, subject, cluster, topic,
/// action, offsets — plus the display name, because a log read by a human six
/// months later should not require a directory lookup to be legible.
#[derive(Debug, Clone, Serialize)]
// camelCase, like every other JSON this project emits. A log line is read by
// the same people and the same tools as the API responses; two conventions in
// one system is one more than anyone can remember.
#[serde(rename_all = "camelCase")]
pub struct Read {
    /// RFC 3339, to milliseconds.
    pub at: String,
    /// The `sub` claim, or `anonymous` on a deployment with no provider.
    pub subject: String,
    /// What the subject renders as.
    pub display_name: String,
    /// Which cluster.
    pub cluster: String,
    /// Which topic.
    pub topic: String,
    /// What they did.
    pub action: Kind,
    /// The seek that was asked for, where there was one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// The lowest offset disclosed, where records were returned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_offset: Option<i64>,
    /// The highest offset disclosed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_offset: Option<i64>,
    /// How many records were disclosed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub records: Option<usize>,
    /// Which partition, for a single-record read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partition: Option<i32>,
}

impl Read {
    /// Begin an entry. The timestamp is taken here, before the write.
    #[must_use]
    pub fn new(
        subject: impl Into<String>,
        display_name: impl Into<String>,
        cluster: impl Into<String>,
        topic: impl Into<String>,
        action: Kind,
    ) -> Self {
        Self {
            at: humantime::format_rfc3339_millis(SystemTime::now()).to_string(),
            subject: subject.into(),
            display_name: display_name.into(),
            cluster: cluster.into(),
            topic: topic.into(),
            action,
            mode: None,
            first_offset: None,
            last_offset: None,
            records: None,
            partition: None,
        }
    }

    /// The seek mode this read was made with.
    #[must_use]
    pub fn with_mode(mut self, mode: impl Into<String>) -> Self {
        self.mode = Some(mode.into());
        self
    }

    /// What was actually returned: the offset range and the count.
    ///
    /// Taken from the rows rather than from the request, because `limit=500`
    /// on a topic holding four records is a claim about the ask and not about
    /// the disclosure.
    #[must_use]
    pub fn with_range(mut self, offsets: impl IntoIterator<Item = i64>, records: usize) -> Self {
        let mut lowest = None;
        let mut highest = None;
        for offset in offsets {
            lowest = Some(lowest.map_or(offset, |current: i64| current.min(offset)));
            highest = Some(highest.map_or(offset, |current: i64| current.max(offset)));
        }
        self.first_offset = lowest;
        self.last_offset = highest;
        self.records = Some(records);
        self
    }

    /// One record, named exactly.
    #[must_use]
    pub fn at_record(mut self, partition: i32, offset: i64) -> Self {
        self.partition = Some(partition);
        self.first_offset = Some(offset);
        self.last_offset = Some(offset);
        self.records = Some(1);
        self
    }
}

/// A write that did not happen.
#[derive(Debug, thiserror::Error)]
#[error("the access audit could not be written: {0}")]
pub struct AuditError(String);

/// The audit log.
pub struct Audit {
    out: Mutex<Box<dyn Write + Send>>,
}

// Hand-written because a boxed writer is not `Debug`, and the workspace denies
// missing `Debug` implementations. Nothing about the sink is worth printing —
// what matters is in the lines it wrote.
impl std::fmt::Debug for Audit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Audit")
    }
}

impl Audit {
    /// One JSON line per read, on stdout.
    #[must_use]
    pub fn to_stdout() -> Self {
        Self {
            out: Mutex::new(Box::new(std::io::stdout())),
        }
    }

    /// Somewhere else — a file, a buffer, a writer that fails on purpose.
    #[must_use]
    pub fn to_writer(out: Box<dyn Write + Send>) -> Self {
        Self {
            out: Mutex::new(out),
        }
    }

    /// Record a disclosure, or refuse to have happened.
    ///
    /// Flushed before returning: a line sitting in a buffer when the process
    /// is killed is a line that was never written, and "the pod restarted" is
    /// not an acceptable reason for a missing audit entry.
    ///
    /// # Errors
    ///
    /// If the entry cannot be serialised, the lock is poisoned, or the write
    /// or flush fails. Every one of those fails the request that caused it.
    pub fn record(&self, entry: &Read) -> Result<(), AuditError> {
        let line = serde_json::to_string(entry)
            .map_err(|error| AuditError(format!("the entry could not be serialised: {error}")))?;

        let mut out = self.out.lock().map_err(|_| {
            // A poisoned lock means a previous write panicked mid-line. The
            // log's integrity is already in question, so every subsequent read
            // fails rather than appending to a file nobody can trust.
            AuditError("the audit writer is poisoned by an earlier failure".to_owned())
        })?;

        writeln!(out, "{line}").map_err(|error| AuditError(error.to_string()))?;
        out.flush().map_err(|error| AuditError(error.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::{Arc, Mutex as StdMutex};

    use super::*;

    /// A writer that hands back what was written to it.
    #[derive(Debug, Clone, Default)]
    struct Shared(Arc<StdMutex<Vec<u8>>>);

    impl Write for Shared {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().map_or(Ok(0), |mut inner| {
                inner.extend_from_slice(buf);
                Ok(buf.len())
            })
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// A writer that always fails, like a closed pipe or a full disk.
    struct Broken;

    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("no space left on device"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_read_is_one_line_of_json_carrying_who_what_and_how_much() {
        let sink = Shared::default();
        let audit = Audit::to_writer(Box::new(sink.clone()));

        audit
            .record(
                &Read::new("sub-1", "Woestebanaan", "kaas", "kaas-canary", Kind::Tail)
                    .with_mode("newest")
                    .with_range([12, 9, 30], 3),
            )
            .expect("a working writer");

        let written = sink.0.lock().expect("not poisoned").clone();
        let text = String::from_utf8(written).expect("utf-8");
        assert_eq!(text.lines().count(), 1, "one read, one line: {text}");

        let entry: serde_json::Value = serde_json::from_str(&text).expect("valid json");
        assert_eq!(entry["subject"], "sub-1");
        assert_eq!(entry["cluster"], "kaas");
        assert_eq!(entry["topic"], "kaas-canary");
        assert_eq!(entry["action"], "tail");
        assert_eq!(entry["mode"], "newest");
        // The range is what was disclosed, not what was asked for.
        assert_eq!(entry["firstOffset"], 9);
        assert_eq!(entry["lastOffset"], 30);
        assert_eq!(entry["records"], 3);
        assert!(
            entry["at"].as_str().is_some_and(|at| at.contains('T')),
            "{entry}"
        );
    }

    #[test]
    fn a_single_record_read_names_its_partition_and_offset() {
        let sink = Shared::default();
        let audit = Audit::to_writer(Box::new(sink.clone()));

        audit
            .record(
                &Read::new("sub-1", "Woestebanaan", "kaas", "orders", Kind::Record)
                    .at_record(3, 4_797_046),
            )
            .expect("a working writer");

        let text = String::from_utf8(sink.0.lock().expect("not poisoned").clone()).expect("utf-8");
        let entry: serde_json::Value = serde_json::from_str(&text).expect("valid json");
        assert_eq!(entry["partition"], 3);
        assert_eq!(entry["firstOffset"], 4_797_046);
        assert_eq!(entry["records"], 1);
    }

    #[test]
    fn a_write_that_fails_is_an_error_rather_than_a_shrug() {
        // The property the whole module exists for: this error is propagated
        // by every caller, so a payload nobody could record is a payload
        // nobody receives.
        let audit = Audit::to_writer(Box::new(Broken));
        let error = audit
            .record(&Read::new("sub", "name", "kaas", "topic", Kind::Stream))
            .expect_err("the writer always fails");
        assert!(error.to_string().contains("no space left"), "{error}");
    }

    #[test]
    fn nothing_is_recorded_for_a_read_that_disclosed_nothing_beyond_its_shape() {
        // An empty window is still a disclosure — somebody looked — so the
        // entry exists, with no offsets rather than fabricated ones.
        let sink = Shared::default();
        let audit = Audit::to_writer(Box::new(sink.clone()));
        audit
            .record(
                &Read::new("sub", "name", "kaas", "empty", Kind::Page)
                    .with_mode("newest")
                    .with_range([], 0),
            )
            .expect("a working writer");

        let text = String::from_utf8(sink.0.lock().expect("not poisoned").clone()).expect("utf-8");
        let entry: serde_json::Value = serde_json::from_str(&text).expect("valid json");
        assert_eq!(entry["records"], 0);
        assert!(entry.get("firstOffset").is_none(), "{entry}");
    }
}
