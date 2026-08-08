//! Protobuf values as JSON.
//!
//! `schema_registry_converter` resolves the descriptor and hands back a
//! `protofish` value tree, which is an upstream type and therefore not
//! something that can reach a `utoipa` schema. This module is the boundary:
//! everything below is `protofish`, everything above is `serde_json`.
//!
//! Two decisions worth stating, because the wire format does not decide them:
//!
//! * **Bytes render as hex**, not as an array of small integers. JSON has no
//!   bytes, and a hundred numbers in a row is not something anyone reads.
//! * **A field the descriptor does not know is kept**, under its number. A
//!   producer that is ahead of the registry writes fields the schema has no
//!   name for, and dropping them would make the record look smaller than it is.

use protofish::context::{Context, Multiplicity};
use protofish::decode::{MessageValue, PackedArray, UnknownValue, Value};
use serde_json::{Map, Value as Json};

/// Render a decoded protobuf message as JSON.
#[must_use]
pub fn to_json(message: &MessageValue, context: &Context) -> Json {
    let info = context.resolve_message(message.msg_ref);
    let mut object = Map::new();

    for field in &message.fields {
        let described = info.get_field(field.number);
        let name = described.map_or_else(
            || format!("{}", field.number),
            |described| described.name.clone(),
        );
        let repeated = described.is_some_and(|described| {
            matches!(
                described.multiplicity,
                Multiplicity::Repeated | Multiplicity::RepeatedPacked
            )
        });

        // One occurrence of a packed field carries the whole list, so it
        // contributes many values where an unpacked one contributes a single
        // value. Both spellings are legal for the same `repeated` field, and
        // the rendering must not be able to tell which the producer chose.
        let value = value_to_json(&field.value, context);
        let contributed = match (&field.value, value) {
            (Value::Packed(_), Json::Array(items)) => items,
            (_, single) => vec![single],
        };
        let repeated = repeated || matches!(field.value, Value::Packed(_));

        match object.entry(name) {
            serde_json::map::Entry::Vacant(slot) => {
                // A repeated field with one occurrence is still a list, and
                // rendering it as a scalar would make the shape depend on the
                // data rather than on the schema.
                slot.insert(match (repeated, contributed) {
                    (true, items) => Json::Array(items),
                    (false, items) => items.into_iter().next().unwrap_or(Json::Null),
                });
            }
            serde_json::map::Entry::Occupied(mut slot) => match slot.get_mut() {
                Json::Array(items) => items.extend(contributed),
                existing => {
                    let first = std::mem::replace(existing, Json::Null);
                    let mut items = vec![first];
                    items.extend(contributed);
                    *existing = Json::Array(items);
                }
            },
        }
    }

    if let Some(garbage) = &message.garbage
        && !garbage.is_empty()
    {
        // Trailing bytes with no field number. Keeping them visible is the
        // difference between "this record is odd" and "this record is fine".
        object.insert("$garbage".to_owned(), Json::String(hex(garbage)));
    }

    Json::Object(object)
}

fn value_to_json(value: &Value, context: &Context) -> Json {
    match value {
        Value::Double(v) => number_f64(*v),
        Value::Float(v) => number_f64(f64::from(*v)),
        Value::Int32(v) | Value::SInt32(v) | Value::SFixed32(v) => Json::from(*v),
        Value::Int64(v) | Value::SInt64(v) | Value::SFixed64(v) => Json::from(*v),
        Value::UInt32(v) | Value::Fixed32(v) => Json::from(*v),
        Value::UInt64(v) | Value::Fixed64(v) => Json::from(*v),
        Value::Bool(v) => Json::Bool(*v),
        Value::String(v) => Json::String(v.clone()),
        Value::Bytes(v) => Json::String(hex(v)),
        Value::Packed(array) => packed_to_json(array),
        Value::Message(message) => to_json(message, context),
        Value::Enum(value) => context
            .resolve_enum(value.enum_ref)
            .get_field_by_value(value.value)
            .map_or_else(
                || Json::from(value.value),
                |field| Json::String(field.name.clone()),
            ),
        // The payload ran out mid-value. Say so rather than guessing at what
        // the rest would have been.
        Value::Incomplete(_, rest) => Json::String(format!("<incomplete: {}>", hex(rest))),
        Value::Unknown(unknown) => unknown_to_json(unknown),
    }
}

fn packed_to_json(array: &PackedArray) -> Json {
    match array {
        PackedArray::Double(items) => Json::Array(items.iter().map(|v| number_f64(*v)).collect()),
        PackedArray::Float(items) => {
            Json::Array(items.iter().map(|v| number_f64(f64::from(*v))).collect())
        }
        PackedArray::Int32(items) | PackedArray::SInt32(items) | PackedArray::SFixed32(items) => {
            Json::Array(items.iter().map(|v| Json::from(*v)).collect())
        }
        PackedArray::Int64(items) | PackedArray::SInt64(items) | PackedArray::SFixed64(items) => {
            Json::Array(items.iter().map(|v| Json::from(*v)).collect())
        }
        PackedArray::UInt32(items) | PackedArray::Fixed32(items) => {
            Json::Array(items.iter().map(|v| Json::from(*v)).collect())
        }
        PackedArray::UInt64(items) | PackedArray::Fixed64(items) => {
            Json::Array(items.iter().map(|v| Json::from(*v)).collect())
        }
        PackedArray::Bool(items) => Json::Array(items.iter().map(|v| Json::Bool(*v)).collect()),
    }
}

fn unknown_to_json(unknown: &UnknownValue) -> Json {
    match unknown {
        // A varint can be wider than a JSON number is exact, so it goes out as
        // a string rather than as a number that silently lost its low bits.
        UnknownValue::Varint(v) => Json::String(format!("{v}")),
        UnknownValue::Fixed64(v) => Json::from(*v),
        UnknownValue::Fixed32(v) => Json::from(*v),
        UnknownValue::VariableLength(bytes) => Json::String(hex(bytes)),
        UnknownValue::Invalid(wire_type, bytes) => {
            Json::String(format!("<invalid wire type {wire_type}: {}>", hex(bytes)))
        }
    }
}

/// A JSON number, or the IEEE name where JSON has none.
///
/// `NaN` and the infinities are not JSON numbers. Rendering them as `null`
/// would make an unrepresentable value look like an absent one.
fn number_f64(value: f64) -> Json {
    serde_json::Number::from_f64(value)
        .map_or_else(|| Json::String(format!("{value}")), Json::Number)
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        // Writing into a String cannot fail.
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use protofish::context::Context;

    /// A descriptor with every shape the converter has to handle: a scalar, a
    /// repeated scalar, bytes, an enum and a nested message.
    const PROTO: &str = r#"
        syntax = "proto3";
        package rs.kaas.test;

        enum Colour {
            UNKNOWN = 0;
            RED = 1;
        }

        message Inner {
            string note = 1;
        }

        message Outer {
            string name = 1;
            repeated int32 counts = 2;
            bytes blob = 3;
            Colour colour = 4;
            Inner inner = 5;
        }
    "#;

    fn context() -> Context {
        Context::parse([PROTO]).unwrap()
    }

    fn varint(mut value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let byte = u8::try_from(value & 0x7f).unwrap();
            value >>= 7;
            if value == 0 {
                out.push(byte);
                return out;
            }
            out.push(byte | 0x80);
        }
    }

    /// A `(field number, wire type)` tag.
    fn tag(number: u64, wire: u8) -> Vec<u8> {
        varint((number << 3) | u64::from(wire))
    }

    fn delimited(number: u64, payload: &[u8]) -> Vec<u8> {
        let mut out = tag(number, 2);
        out.extend(varint(u64::try_from(payload.len()).unwrap()));
        out.extend_from_slice(payload);
        out
    }

    #[test]
    fn a_message_renders_with_field_names_from_the_descriptor() {
        let context = context();
        let outer = context.get_message("rs.kaas.test.Outer").unwrap();

        let mut bytes = delimited(1, b"widget");
        // counts: packed, which is what proto3 does with repeated scalars.
        bytes.extend(delimited(2, &[7, 9]));
        bytes.extend(delimited(3, &[0xde, 0xad]));
        // colour: RED
        bytes.extend(tag(4, 0));
        bytes.push(1);
        bytes.extend(delimited(5, &delimited(1, b"hi")));

        let value = outer.decode(&bytes, &context);
        let json = to_json(&value, &context);

        assert_eq!(json["name"], serde_json::json!("widget"));
        assert_eq!(json["counts"], serde_json::json!([7, 9]));
        // Bytes are hex, because a hundred small integers is not a rendering.
        assert_eq!(json["blob"], serde_json::json!("dead"));
        // An enum is its name, not its number.
        assert_eq!(json["colour"], serde_json::json!("RED"));
        assert_eq!(json["inner"]["note"], serde_json::json!("hi"));
    }

    #[test]
    fn a_field_the_descriptor_does_not_know_is_kept_under_its_number() {
        // A producer ahead of the registry. Dropping the field would make the
        // record look smaller than it is, which is worse than an odd key.
        let context = context();
        let outer = context.get_message("rs.kaas.test.Outer").unwrap();

        let mut bytes = delimited(1, b"widget");
        bytes.extend(tag(99, 0));
        bytes.push(42);

        let json = to_json(&outer.decode(&bytes, &context), &context);
        assert_eq!(json["name"], serde_json::json!("widget"));
        assert_eq!(json["99"], serde_json::json!("42"));
    }

    #[test]
    fn a_repeated_field_with_one_value_is_still_a_list() {
        // The shape has to come from the schema, not from how many values this
        // particular record happened to carry.
        let context = context();
        let outer = context.get_message("rs.kaas.test.Outer").unwrap();

        let json = to_json(&outer.decode(&delimited(2, &[7]), &context), &context);
        assert_eq!(json["counts"], serde_json::json!([7]));
    }

    #[test]
    fn values_json_cannot_hold_are_named_rather_than_nulled() {
        assert_eq!(number_f64(1.5), serde_json::json!(1.5));
        assert_eq!(number_f64(f64::NAN), serde_json::json!("NaN"));
        assert_eq!(number_f64(f64::INFINITY), serde_json::json!("inf"));
    }
}
