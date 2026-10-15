mod bytes;
mod regression;
mod unicode;

use serde::Deserialize;

use crate::{
    from_str, get, to_array_iter, to_object_iter, to_string, Deserializer, LazyValue, Value,
};

use super::MAX_ALLOWED_DEPTH;

fn nested(depth: usize, objects: bool) -> String {
    let (open, close) = if objects {
        ("{\"x\":", "}")
    } else {
        ("[", "]")
    };
    format!("{}0{}", open.repeat(depth), close.repeat(depth))
}

#[test]
fn dom_boundaries_parse_serialize_and_drop_on_the_default_stack() {
    let limit = usize::from(MAX_ALLOWED_DEPTH) - 1;
    for objects in [false, true] {
        for depth in [32, 128, limit, limit + 1, 32768] {
            let input = nested(depth, objects);
            for raw in [false, true] {
                let mut parser = Deserializer::from_slice(input.as_bytes());
                if raw {
                    parser = parser.use_rawnumber();
                }
                let result = parser.deserialize::<Value>();
                assert_eq!(result.is_ok(), depth <= limit);
                if let Ok(value) = result {
                    parser.end().expect("complete fixture");
                    assert_eq!(to_string(&value).expect("serialization"), input);
                    drop(value);
                }
            }
        }
    }
}

#[derive(Deserialize)]
struct Ignoring {
    model: String,
}

#[derive(Deserialize)]
struct Embedded {
    value: Value,
}

#[test]
fn ignored_embedded_and_lazy_values_share_the_parent_depth_budget() {
    let limit = usize::from(MAX_ALLOWED_DEPTH) - 1;
    for objects in [false, true] {
        for depth in [32, limit - 1, limit, 32768] {
            let child = nested(depth, objects);
            let input = format!(r#"{{"model":"fixture","value":{child}}}"#);
            let ignored = from_str::<Ignoring>(&input);
            let embedded = from_str::<Embedded>(&input);
            let lazy = from_str::<LazyValue<'_>>(&input);
            assert_eq!(ignored.is_ok(), depth < limit);
            assert_eq!(embedded.is_ok(), depth < limit);
            assert_eq!(lazy.is_ok(), depth < limit);
            if let Ok(ignored) = ignored {
                assert_eq!(ignored.model, "fixture");
            }
            if let Ok(embedded) = embedded {
                assert_eq!(to_string(&embedded.value).expect("serialization"), child);
            }
        }
    }
}

#[test]
fn checked_iterators_and_get_account_for_the_enclosing_container() {
    let limit = usize::from(MAX_ALLOWED_DEPTH) - 1;
    for depth in [limit - 1, limit, 32768] {
        let child = nested(depth, false);
        let object = format!(r#"{{"value":{child}}}"#);
        let array = format!("[{child}]");
        assert_eq!(
            to_object_iter(object.as_str())
                .next()
                .expect("entry")
                .is_ok(),
            depth < limit
        );
        assert_eq!(
            to_array_iter(array.as_str())
                .next()
                .expect("element")
                .is_ok(),
            depth < limit
        );
        assert_eq!(get(object.as_str(), ["value"]).is_ok(), depth < limit);
    }
}
