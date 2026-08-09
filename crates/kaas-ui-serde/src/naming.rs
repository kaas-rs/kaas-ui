//! Which naming strategy a subject was registered under, and what that gives
//! back.
//!
//! There are three, and a subject name is all three of them at once until the
//! schema is consulted:
//!
//! | strategy | subject | the topic |
//! |---|---|---|
//! | `TopicNameStrategy` | `{topic}-value`, `{topic}-key` | from the name alone |
//! | `TopicRecordNameStrategy` | `{topic}-{record}` | once the record name is known |
//! | `RecordNameStrategy` | `{record}` | there was never one in it |
//!
//! The middle row is what this module exists for. `orders-com.acme.Order` is a
//! topic and a record glued with the same `-` that `orders-value` uses, and
//! nothing in the string says where the seam is. The schema says: it declares
//! the record's fully-qualified name, and the subject ends with it. Take that
//! off and what remains is the topic, exactly, with nothing guessed.
//!
//! `RecordNameStrategy` is the honest absence rather than a gap. One record is
//! produced to whatever topics produce it and that mapping lives in the
//! records, so "this names a record, not a topic" is the whole answer — a
//! different sentence from "this could not be read", and the UI must not
//! render them the same way.

use serde::Serialize;
use serde_json::Value as Json;
use utoipa::ToSchema;

use crate::codec::SchemaFormat;

/// Which of the three formed a subject name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum NamingStrategy {
    /// `{topic}-value` or `{topic}-key`.
    TopicName,
    /// `{topic}-{record}`.
    TopicRecordName,
    /// `{record}`, and no topic anywhere in it.
    RecordName,
    /// None of them — a subject registered by hand, or under a strategy
    /// somebody wrote themselves.
    Unrecognized,
}

/// A subject name, read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubjectNaming {
    /// Which strategy the name fits.
    pub strategy: NamingStrategy,
    /// The topic the subject names, where the strategy carries one.
    ///
    /// `None` under [`NamingStrategy::RecordName`] because there is no topic in
    /// the name, and under [`NamingStrategy::Unrecognized`] because there is no
    /// rule to read it by. The two are not the same fact and `strategy` is what
    /// tells them apart.
    pub topic: Option<String>,
    /// The fully-qualified name the schema declares, where it declares one.
    pub record_name: Option<String>,
}

impl SubjectNaming {
    /// Read a subject name, given what its newest schema declares.
    ///
    /// `record_name` is `None` on a schema that declares no name, and on a
    /// registry that would not hand the schema over. Both degrade to the same
    /// place: the two topic-bearing strategies that need no schema still
    /// resolve, and `{topic}-{record}` reads as [`NamingStrategy::Unrecognized`]
    /// rather than being split at a guessed `-`.
    #[must_use]
    pub fn of(subject: &str, record_name: Option<&str>) -> Self {
        let record = record_name.filter(|name| !name.is_empty());

        // Before the suffixes, and unambiguously so: `-` is not a legal
        // character in an Avro or Protobuf name and a JSON Schema title used as
        // one, so a subject that *is* a record name cannot also be a
        // `{topic}-value`, and a `{topic}-{record}` seam cannot fall inside the
        // record half.
        if let Some(name) = record {
            if subject == name {
                return Self::named(NamingStrategy::RecordName, None, name);
            }
            if let Some(topic) = subject
                .strip_suffix(name)
                .and_then(|head| head.strip_suffix('-'))
                && !topic.is_empty()
            {
                return Self::named(NamingStrategy::TopicRecordName, Some(topic), name);
            }
        }

        for suffix in ["-value", "-key"] {
            if let Some(topic) = subject.strip_suffix(suffix)
                && !topic.is_empty()
            {
                return Self {
                    strategy: NamingStrategy::TopicName,
                    topic: Some(topic.to_owned()),
                    record_name: record.map(str::to_owned),
                };
            }
        }

        Self {
            strategy: NamingStrategy::Unrecognized,
            topic: None,
            record_name: record.map(str::to_owned),
        }
    }

    fn named(strategy: NamingStrategy, topic: Option<&str>, record_name: &str) -> Self {
        Self {
            strategy,
            topic: topic.map(str::to_owned),
            record_name: Some(record_name.to_owned()),
        }
    }
}

/// The fully-qualified name a schema declares, where it declares one.
///
/// This is the name the two record strategies register under — Confluent's
/// `ParsedSchema#name()`, one format at a time:
///
/// * **Avro** — `namespace` and `name`, or `name` alone where it already
///   carries dots. A union or a bare primitive declares nothing.
/// * **Protobuf** — `package` and the first top-level `message`.
/// * **JSON Schema** — `title`, which is the only place a JSON schema puts a
///   name the serializer can use.
///
/// `None` is a normal answer, and it costs only the topic *link*: a subject
/// still lists, decodes and diffs without it.
///
/// The payload side reaches the same fact from the other end —
/// [`SchemaRef::name`](crate::codec::SchemaRef::name) is what the decoder that
/// just decoded a record reports. There is no decoder here, only the text, so
/// the two cannot share an implementation; they must agree all the same.
#[must_use]
pub fn declared_name(format: SchemaFormat, schema: &str) -> Option<String> {
    match format {
        SchemaFormat::Avro => avro_name(schema),
        SchemaFormat::Json => json_title(schema),
        SchemaFormat::Protobuf => protobuf_name(schema),
    }
}

/// `namespace` + `name`, by Avro's own qualification rule.
fn avro_name(schema: &str) -> Option<String> {
    let schema: Json = serde_json::from_str(schema).ok()?;
    let name = schema
        .get("name")?
        .as_str()
        .filter(|name| !name.is_empty())?;

    // A name containing a dot is already a full name and the namespace beside
    // it is ignored. That is the spec, not a shortcut around the join.
    if name.contains('.') {
        return Some(name.to_owned());
    }

    match schema.get("namespace").and_then(Json::as_str) {
        Some(namespace) if !namespace.is_empty() => Some(format!("{namespace}.{name}")),
        _ => Some(name.to_owned()),
    }
}

/// `title`, and only a non-empty one.
fn json_title(schema: &str) -> Option<String> {
    let schema: Json = serde_json::from_str(schema).ok()?;
    schema
        .get("title")?
        .as_str()
        .filter(|title| !title.is_empty())
        .map(str::to_owned)
}

/// `package` + the first message declared at the top level.
///
/// A text scan rather than a parse: a `.proto` that imports another cannot be
/// parsed alone, and those are the schemas most worth linking. Nothing here
/// needs the imports resolved — a package statement and a message name are
/// lexical.
fn protobuf_name(schema: &str) -> Option<String> {
    let text = without_comments(schema);
    let words = words(&text);

    let mut package = None;
    let mut message = None;
    let mut previous: Option<&str> = None;
    for &(depth, ref word) in &words {
        // Only at the top level: a nested `message` is not what the serializer
        // named the subject after, and `option java_package` is a different
        // word from `package` precisely because a word is a maximal run.
        if depth == 0 {
            match previous {
                Some("package") if package.is_none() => package = Some(word.clone()),
                Some("message") if message.is_none() => message = Some(word.clone()),
                _ => {}
            }
        }
        previous = Some(word);
    }

    let message = message?;
    Some(match package {
        Some(package) => format!("{package}.{message}"),
        None => message,
    })
}

/// Identifier runs, each with the brace depth it was found at.
///
/// `.` belongs to a word so `com.acme` stays one, and string contents are
/// skipped whole — an `option` value holding a brace would otherwise move
/// every message after it out of the top level.
fn words(text: &str) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = Vec::new();
    let mut word = String::new();
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut escaped = false;

    for character in text.chars() {
        if let Some(open) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == open {
                quote = None;
            }
            continue;
        }

        match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '.' => word.push(character),
            other => {
                if !word.is_empty() {
                    out.push((depth, std::mem::take(&mut word)));
                }
                match other {
                    '"' | '\'' => quote = Some(other),
                    '{' => depth += 1,
                    '}' => depth = depth.saturating_sub(1),
                    _ => {}
                }
            }
        }
    }
    if !word.is_empty() {
        out.push((depth, word));
    }
    out
}

/// Drop `//` and `/* */`, leaving string literals alone.
fn without_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    let mut quote: Option<char> = None;
    let mut escaped = false;

    while let Some(character) = characters.next() {
        if let Some(open) = quote {
            out.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == open {
                quote = None;
            }
            continue;
        }

        match character {
            '"' | '\'' => {
                quote = Some(character);
                out.push(character);
            }
            '/' if characters.peek() == Some(&'/') => {
                // The newline stays: it is what ends the statement the comment
                // was hanging off.
                for skipped in characters.by_ref() {
                    if skipped == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if characters.peek() == Some(&'*') => {
                characters.next();
                let mut previous = '\0';
                for skipped in characters.by_ref() {
                    if previous == '*' && skipped == '/' {
                        break;
                    }
                    previous = skipped;
                }
                // A separator, so `message/*x*/Foo` does not become one word.
                out.push(' ');
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORDER_AVRO: &str =
        r#"{"type":"record","name":"Order","namespace":"com.acme","fields":[]}"#;

    #[test]
    fn a_value_suffix_is_the_topic_name_strategy() {
        let naming = SubjectNaming::of("orders-value", Some("com.acme.Order"));
        assert_eq!(naming.strategy, NamingStrategy::TopicName);
        assert_eq!(naming.topic.as_deref(), Some("orders"));
        // Still reported: it is what the schema declares either way.
        assert_eq!(naming.record_name.as_deref(), Some("com.acme.Order"));

        let key = SubjectNaming::of("orders-key", None);
        assert_eq!(key.strategy, NamingStrategy::TopicName);
        assert_eq!(key.topic.as_deref(), Some("orders"));
    }

    #[test]
    fn a_subject_that_is_the_record_name_names_no_topic() {
        let naming = SubjectNaming::of("com.acme.Order", Some("com.acme.Order"));
        assert_eq!(naming.strategy, NamingStrategy::RecordName);
        // The point of the whole exercise: no topic, and that is an answer.
        assert_eq!(naming.topic, None);
    }

    #[test]
    fn the_record_name_is_where_the_topic_record_seam_is() {
        let naming = SubjectNaming::of("orders-com.acme.Order", Some("com.acme.Order"));
        assert_eq!(naming.strategy, NamingStrategy::TopicRecordName);
        assert_eq!(naming.topic.as_deref(), Some("orders"));
    }

    #[test]
    fn a_topic_with_hyphens_survives_the_seam() {
        // Split at the first `-` this would be `orders`, and the link would go
        // to a topic that does not exist. The record name is what makes it exact.
        let naming = SubjectNaming::of("orders-eu-west-1-com.acme.Order", Some("com.acme.Order"));
        assert_eq!(naming.strategy, NamingStrategy::TopicRecordName);
        assert_eq!(naming.topic.as_deref(), Some("orders-eu-west-1"));
    }

    #[test]
    fn without_a_record_name_only_the_suffixes_read() {
        // The registry was down, or the schema declares nothing. A guessed
        // seam is worse than no link, so this is `Unrecognized`, not a topic.
        let naming = SubjectNaming::of("orders-com.acme.Order", None);
        assert_eq!(naming.strategy, NamingStrategy::Unrecognized);
        assert_eq!(naming.topic, None);

        let suffixed = SubjectNaming::of("orders-value", None);
        assert_eq!(suffixed.strategy, NamingStrategy::TopicName);
    }

    #[test]
    fn a_subject_following_no_strategy_is_not_forced_into_one() {
        let naming = SubjectNaming::of("legacy_orders", Some("com.acme.Order"));
        assert_eq!(naming.strategy, NamingStrategy::Unrecognized);
        assert_eq!(naming.topic, None);
        assert_eq!(naming.record_name.as_deref(), Some("com.acme.Order"));
    }

    #[test]
    fn a_bare_suffix_is_not_a_topic() {
        // `-value` would otherwise be topic "" and link to nothing.
        assert_eq!(
            SubjectNaming::of("-value", None).strategy,
            NamingStrategy::Unrecognized
        );
        assert_eq!(
            SubjectNaming::of("com.acme.Order", Some("com.acme.Order")).topic,
            None
        );
    }

    #[test]
    fn an_avro_name_joins_its_namespace() {
        assert_eq!(
            declared_name(SchemaFormat::Avro, ORDER_AVRO).as_deref(),
            Some("com.acme.Order")
        );
    }

    #[test]
    fn an_avro_name_already_qualified_ignores_the_namespace() {
        let schema = r#"{"type":"record","name":"com.acme.Order","namespace":"ignored"}"#;
        assert_eq!(
            declared_name(SchemaFormat::Avro, schema).as_deref(),
            Some("com.acme.Order")
        );
    }

    #[test]
    fn an_avro_schema_with_no_namespace_is_its_bare_name() {
        let schema = r#"{"type":"record","name":"Order"}"#;
        assert_eq!(
            declared_name(SchemaFormat::Avro, schema).as_deref(),
            Some("Order")
        );
    }

    #[test]
    fn an_unnamed_avro_schema_declares_nothing() {
        // A top-level union, and a bare primitive. Neither can be registered
        // under a record strategy, so neither has a name to take off a subject.
        assert_eq!(
            declared_name(SchemaFormat::Avro, r#"["null","string"]"#),
            None
        );
        assert_eq!(
            declared_name(SchemaFormat::Avro, r#"{"type":"string"}"#),
            None
        );
        assert_eq!(declared_name(SchemaFormat::Avro, "not json at all"), None);
    }

    #[test]
    fn a_json_schema_is_named_by_its_title() {
        let schema = r#"{"title":"com.acme.Order","type":"object"}"#;
        assert_eq!(
            declared_name(SchemaFormat::Json, schema).as_deref(),
            Some("com.acme.Order")
        );
        assert_eq!(
            declared_name(SchemaFormat::Json, r#"{"type":"object"}"#),
            None
        );
    }

    #[test]
    fn a_proto_message_is_named_by_its_package() {
        let schema =
            "syntax = \"proto3\";\npackage com.acme;\n\nmessage Order {\n  string id = 1;\n}\n";
        assert_eq!(
            declared_name(SchemaFormat::Protobuf, schema).as_deref(),
            Some("com.acme.Order")
        );
    }

    #[test]
    fn a_proto_file_with_no_package_is_the_bare_message() {
        assert_eq!(
            declared_name(SchemaFormat::Protobuf, "message Order { string id = 1; }").as_deref(),
            Some("Order")
        );
        assert_eq!(
            declared_name(SchemaFormat::Protobuf, "syntax = \"proto3\";"),
            None
        );
    }

    #[test]
    fn a_nested_message_is_not_the_first_message() {
        let schema = "package com.acme;\nmessage Order {\n  message Line { string sku = 1; }\n  Line line = 1;\n}\nmessage Other {}\n";
        assert_eq!(
            declared_name(SchemaFormat::Protobuf, schema).as_deref(),
            Some("com.acme.Order")
        );
    }

    #[test]
    fn comments_and_options_do_not_name_the_message() {
        // `java_package` is a different word, the commented-out message is not
        // one, and the brace inside the string must not push `Order` down a level.
        let schema = "syntax = \"proto3\";\n\
             option java_package = \"com.acme.{proto}\";\n\
             // message Ghost {}\n\
             /* message AlsoGhost {\n */\n\
             package com.acme;\n\
             message Order { string id = 1; }\n";
        assert_eq!(
            declared_name(SchemaFormat::Protobuf, schema).as_deref(),
            Some("com.acme.Order")
        );
    }

    #[test]
    fn a_schema_and_its_subject_read_together() {
        // The whole path, as the API walks it.
        let name = declared_name(SchemaFormat::Avro, ORDER_AVRO);
        let naming = SubjectNaming::of("orders-com.acme.Order", name.as_deref());
        assert_eq!(naming.strategy, NamingStrategy::TopicRecordName);
        assert_eq!(naming.topic.as_deref(), Some("orders"));
    }
}
