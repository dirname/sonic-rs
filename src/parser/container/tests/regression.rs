use std::{io::ErrorKind, iter::repeat_n, ops::Not};

use serde::de::IgnoredAny;

use super::{nested, Embedded, Ignoring};
use crate::{
    from_slice, from_str, get, get_many,
    parser::{Parser, MAX_ALLOWED_DEPTH},
    reader::Read,
    to_array_iter, to_object_iter, to_string, Deserializer, JsonValueTrait, LazyValue,
    OwnedLazyValue, PointerTree, Value,
};

#[test]
fn invalid_utf8_is_rejected_before_dom_or_lazy_string_construction() {
    let invalid: &[&[u8]] = &[
        b"\xff",
        b"\x80",
        b"\xc0\xaf",
        b"\xe2\x82",
        b"\xed\xa0\x80",
        b"\xf4\x90\x80\x80",
    ];
    for bytes in invalid {
        for width in [0, 31, 64] {
            let prefix = "a".repeat(width);
            for text in [
                format!(r#""{prefix}#""#),
                format!(r#""{prefix}\n#\t""#),
                format!(r#"{{"{prefix}#":0}}"#),
                format!(r#"{{"model":"fixture","value":["{prefix}#"]}}"#),
            ] {
                let split = text.find('#').expect("fixture marker");
                let input = [
                    &text.as_bytes()[..split],
                    bytes,
                    &text.as_bytes()[split + 1..],
                ]
                .concat();
                for raw in [false, true] {
                    let mut de = Deserializer::from_slice(&input);
                    if raw {
                        de = de.use_rawnumber();
                    }
                    assert!(de.deserialize::<Value>().is_err(), "{input:?}");
                }
                assert!(Deserializer::from_slice(&input)
                    .deserialize::<IgnoredAny>()
                    .is_err());
                assert!(Deserializer::from_slice(&input)
                    .deserialize::<LazyValue<'_>>()
                    .is_err());
                assert!(Deserializer::from_slice(&input)
                    .deserialize::<OwnedLazyValue>()
                    .is_err());
                // Raw lazy strings cannot apply lossy replacement while preserving their input.
                assert!(Deserializer::from_slice(&input)
                    .utf8_lossy()
                    .deserialize::<LazyValue<'_>>()
                    .is_err());
                assert!(Deserializer::from_slice(&input)
                    .utf8_lossy()
                    .deserialize::<OwnedLazyValue>()
                    .is_err());
                if cfg!(feature = "utf8_lossy").not() {
                    assert!(from_slice::<Value>(&input).is_err());
                    assert!(from_slice::<IgnoredAny>(&input).is_err());
                }
            }
        }
        let input = [
            b"{\"model\":\"fixture\",\"value\":[\"".as_slice(),
            bytes,
            b"\"]}",
        ]
        .concat();
        assert!(Deserializer::from_slice(&input)
            .deserialize::<Ignoring>()
            .is_err());
        assert!(Deserializer::from_slice(&input)
            .deserialize::<Embedded>()
            .is_err());
        assert!(get(input.as_slice(), ["value"]).is_err());
        let mut tree = PointerTree::new();
        tree.add_path(["value"]);
        assert!(get_many(input.as_slice(), &tree).is_err());
        assert!(to_object_iter(input.as_slice())
            .next()
            .expect("invalid entry")
            .is_err());
        let array = [b"[\"".as_slice(), bytes, b"\"]"].concat();
        assert!(to_array_iter(array.as_slice())
            .next()
            .expect("invalid element")
            .is_err());
    }
}

#[test]
fn lossy_dom_preserves_stream_offsets_and_valid_prefixes() {
    let mut de = Deserializer::from_slice(b"\"ok\" \"\xff\"");
    let first: Value = de.deserialize().expect("valid first document");
    assert_eq!(first.as_str(), Some("ok"));
    assert!(de.deserialize::<Value>().is_err());
    assert_eq!(to_string(&first).expect("retained document"), "\"ok\"");

    let mut de = Deserializer::from_slice(b"\"\xff\" \"\xe2\x82\" 1e-400")
        .utf8_lossy()
        .use_rawnumber();
    let first: Value = de.deserialize().expect("lossy first document");
    let second: Value = de.deserialize().expect("lossy second document");
    let number: Value = de.deserialize().expect("third document");
    de.end().expect("original byte offset reached EOF");
    drop(de);
    assert_eq!(first.as_str(), Some("\u{fffd}"));
    assert_eq!(second.as_str(), Some("\u{fffd}"));
    assert_eq!(to_string(&number).expect("raw number"), "1e-400");

    let mut de =
        Deserializer::from_slice(b"{\"model\":\"fixture\",\"unknown\":\"\xff\"}").utf8_lossy();
    assert_eq!(
        de.deserialize::<Ignoring>()
            .expect("lossy ignored field")
            .model,
        "fixture"
    );
    de.end().expect("ignored lossy document consumed");

    for input in [b"\xff".as_slice(), b"[\xff]", b"{\xff:0}"] {
        assert!(Deserializer::from_slice(input)
            .utf8_lossy()
            .deserialize::<Value>()
            .is_err());
    }
}

#[test]
fn truncated_containers_are_eof_but_invalid_tokens_remain_syntax() {
    for input in [
        "",
        " ",
        "{",
        "[",
        "[0",
        "[{}",
        "{\"x\":0",
        "{\"x\":{}",
        "{\"x\":0,",
        "[0,",
    ] {
        for raw in [false, true] {
            let mut de = Deserializer::from_str(input);
            if raw {
                de = de.use_rawnumber();
            }
            let error = de.deserialize::<Value>().expect_err("truncated DOM");
            assert!(error.is_eof(), "{input:?}: {error}");
            assert_eq!(std::io::Error::from(error).kind(), ErrorKind::UnexpectedEof);
        }
        assert!(from_str::<IgnoredAny>(input)
            .expect_err("truncated ignored value")
            .is_eof());
        assert!(from_str::<LazyValue<'_>>(input)
            .expect_err("truncated lazy value")
            .is_eof());
        let wrapped = format!("{{\"value\":{input}");
        assert!(from_str::<Embedded>(&wrapped)
            .err()
            .expect("truncated embedded value")
            .is_eof());
    }
    for input in ["{?", "[0 ?", "{\"x\":0 ?", "[1,]", "{\"x\":}"] {
        let error = from_str::<Value>(input).expect_err("invalid token");
        assert!(error.is_syntax(), "{input:?}: {error}");
        assert_eq!(std::io::Error::from(error).kind(), ErrorKind::InvalidData);
        assert!(from_str::<IgnoredAny>(input)
            .expect_err("invalid ignored value")
            .is_syntax());
        assert!(from_str::<LazyValue<'_>>(input)
            .expect_err("invalid lazy value")
            .is_syntax());
    }
}

#[test]
fn get_many_counts_selected_and_skipped_ancestors() {
    for objects in [false, true] {
        for depth in [32, 128, 253, 254, 255, 32768] {
            let child = nested(depth, objects);
            let mut tree = PointerTree::new();
            tree.add_path(["value"]);
            let input = format!(r#"{{"value":{child}}}"#);
            let result = get_many(input.as_str(), &tree);
            assert_eq!(result.is_ok(), depth < 254, "{depth}, {objects}");
            if let Ok(values) = result {
                assert_eq!(
                    values[0].as_ref().expect("selected child").as_raw_str(),
                    child
                );
            }
            for input in [
                format!(r#"{{"skip":{child},"value":0}}"#),
                format!(r#"[{{"skip":{child},"value":0}}]"#),
            ] {
                let mut tree = PointerTree::new();
                if input.starts_with('[') {
                    tree.add_path(crate::pointer![0, "value"]);
                } else {
                    tree.add_path(crate::pointer!["value"]);
                }
                let ancestors = if input.starts_with('[') { 2 } else { 1 };
                assert_eq!(
                    get_many(input.as_str(), &tree).is_ok(),
                    depth + ancestors <= 254
                );
            }
        }
    }
}

#[test]
fn deeply_selected_paths_use_the_default_stack_and_restore_sibling_budget() {
    for depth in [32, 128, 254, 255, 512] {
        for objects in [false, true] {
            let input = nested(depth, objects);
            let mut tree = PointerTree::new();
            if objects {
                tree.add_path(repeat_n("x", depth));
            } else {
                tree.add_path(repeat_n(0_usize, depth));
            }
            let result = get_many(input.as_str(), &tree);
            assert_eq!(result.is_ok(), depth <= 254, "{depth}, {objects}");
            if let Ok(values) = result {
                assert_eq!(values[0].as_ref().expect("selected leaf").as_raw_str(), "0");
            }
        }
    }
    let value = nested(252, true);
    let input = format!(r#"{{"a":{{"x":{value}}},"b":{{"x":{value}}}}}"#);
    let mut tree = PointerTree::new();
    tree.add_path(["a", "x"]);
    tree.add_path(["b", "x"]);
    let values = get_many(input.as_str(), &tree).expect("siblings share, not consume, the budget");
    for value in values {
        assert!(value.is_some());
    }
}

#[test]
fn selected_keys_are_validated_before_string_construction() {
    for input in [
        b"{\"\xff\":0}".as_slice(),
        b"{\"a\\n\xff\":0}",
        b"{\"value\":{\"\xff\":0}}",
    ] {
        assert!(get(input, ["value"]).is_err());
        let mut tree = PointerTree::new();
        tree.add_path(["value", "x"]);
        assert!(get_many(input, &tree).is_err());
        let schema = from_str::<Value>(r#"{"value":{"x":0}}"#).expect("schema fixture");
        assert!(crate::get_by_schema(input, schema).is_err());
    }
    // An invalid later document must not invalidate an already selected valid prefix.
    let input = b"{\"value\":0} \xff";
    assert_eq!(
        get(input.as_slice(), ["value"])
            .expect("prefix")
            .as_raw_str(),
        "0"
    );
    let mut tree = PointerTree::new();
    tree.add_path(["value"]);
    assert!(get_many(input.as_slice(), &tree).is_ok());
    let schema = from_str::<Value>(r#"{"value":null}"#).expect("schema fixture");
    assert!(crate::get_by_schema(input.as_slice(), schema).is_ok());
}

#[test]
fn get_many_restores_budget_on_errors_and_handles_escaped_paths() {
    let mut tree = PointerTree::new();
    tree.add_path(["a\n", "x"]);
    let values = get_many(r#"{"a\n":{"x":1}}"#, &tree).expect("escaped key path");
    assert_eq!(
        values[0].as_ref().expect("selected value").as_raw_str(),
        "1"
    );
    for input in [
        r#"{"a\n":{"x":}}"#.to_owned(),
        format!(r#"{{"a\n":{{"x":{}}}}}"#, nested(253, false)),
    ] {
        let mut parser = Parser::new(Read::from(input.as_str()));
        assert!(parser.get_many(&tree, true).is_err());
        assert_eq!(parser.remaining_depth, MAX_ALLOWED_DEPTH);
    }
}

#[test]
fn schema_selection_accounts_for_selected_and_skipped_containers() {
    for depth in [32, 128, 254, 255, 512] {
        let input = nested(depth, true);
        let mut schema = Value::from(0);
        for _ in 0..depth {
            let mut object = crate::Object::new();
            object.insert("x", schema);
            schema = Value::from(object);
        }
        let result = crate::get_by_schema(input.as_str(), schema);
        assert_eq!(result.is_ok(), depth <= 254);
        if let Ok(value) = result {
            assert_eq!(to_string(&value).expect("selected schema"), input);
        }
    }
    for depth in [253, 254, 32768] {
        let input = format!(r#"{{"skip":{},"x":0}}"#, nested(depth, false));
        let mut schema = from_str::<Value>(r#"{"x":null}"#).expect("schema");
        let mut parser = Parser::new(Read::from(input.as_str()));
        assert_eq!(parser.get_by_schema(&mut schema).is_ok(), depth < 254);
        assert_eq!(parser.remaining_depth, MAX_ALLOWED_DEPTH);
    }
}
