use serde::de::IgnoredAny;

use crate::{
    get, get_many, to_array_iter, to_object_iter, Deserializer, LazyValue, OwnedLazyValue,
    PointerTree, Value,
};

fn accepts<T: serde::de::DeserializeOwned>(input: &str, lossy: bool) -> bool {
    let mut de = Deserializer::from_str(input);
    if lossy {
        de = de.utf8_lossy();
    }
    de.deserialize::<T>().and_then(|_| de.end()).is_ok()
}

#[test]
fn checked_skipping_validates_unicode_escapes_without_rewriting_raw_text() {
    for (escaped, valid) in [
        (r"\uZZZZ", false),
        (r"\u00x0", false),
        (r"\u000g", false),
        (r"\uD800", false),
        (r"\uDC00", false),
        (r"\uD800\u0041", false),
        (r"\uD800\uD800", false),
        (r"\uD800\uZZZZ", false),
        (r"\q", false),
        (r"\u0000", true),
        (r"\uD7ff", true),
        (r"\ue000", true),
        (r"\uffff", true),
        (r"\uDBFF\uDFFF", true),
        (r"\ud83D\uDe00", true),
        (r"\\uZZZZ", true),
        (r#"\"\\\/\b\f\n\r\t"#, true),
    ] {
        for width in [0, 30, 31, 32, 63, 64, 96] {
            let prefix = "a".repeat(width);
            let string = format!(r#""{prefix}{escaped}""#);
            for input in [string.clone(), format!(r#"[{{"x":{string}}}]"#)] {
                assert_eq!(accepts::<Value>(&input, false), valid, "DOM {input}");
                assert_eq!(accepts::<IgnoredAny>(&input, false), valid, "skip {input}");
                assert_eq!(
                    accepts::<OwnedLazyValue>(&input, false),
                    valid,
                    "owned {input}"
                );
                let mut de = Deserializer::from_str(&input);
                let result = de.deserialize::<LazyValue<'_>>();
                assert_eq!(result.is_ok(), valid, "lazy {input}");
                if let Ok(value) = result {
                    de.end().expect("complete lazy value");
                    assert_eq!(value.as_raw_str(), input);
                }
            }
            let object = format!(r#"{{"skip":{string},"value":0}}"#);
            assert_eq!(get(object.as_str(), ["value"]).is_ok(), valid);
            let mut tree = PointerTree::new();
            tree.add_path(["value"]);
            assert_eq!(get_many(object.as_str(), &tree).is_ok(), valid);
            assert_eq!(
                to_object_iter(object.as_str()).all(|item| item.is_ok()),
                valid
            );
            assert_eq!(
                to_array_iter(format!("[{string}]").as_str()).all(|item| item.is_ok()),
                valid
            );
        }
    }
}

#[test]
fn truncated_escapes_fail_and_lossy_surrogates_match_string_deserialization() {
    for input in [
        r#""\"#,
        r#""\u"#,
        r#""\u0"#,
        r#""\u00"#,
        r#""\u000"#,
        r#""\uD800"#,
    ] {
        assert!(Deserializer::from_str(input)
            .deserialize::<IgnoredAny>()
            .is_err());
        assert!(Deserializer::from_str(input)
            .deserialize::<Value>()
            .is_err());
    }
    for escaped in [
        r"\uD800",
        r"\uDC00",
        r"\uD800a",
        r"\uD800\u0041",
        r"\uD800\uD83D\uDE00",
    ] {
        let input = format!(r#""{escaped}""#);
        assert_eq!(
            accepts::<IgnoredAny>(&input, true),
            accepts::<String>(&input, true)
        );
    }
}

#[test]
fn every_single_utf16_unit_has_the_same_strict_skip_and_decode_acceptance() {
    for unit in 0..=u16::MAX {
        let input = format!(r#""\u{unit:04X}""#);
        let valid = char::from_u32(u32::from(unit)).is_some();
        assert_eq!(accepts::<IgnoredAny>(&input, false), valid, "{input}");
        assert_eq!(accepts::<String>(&input, false), valid, "{input}");
    }
}
