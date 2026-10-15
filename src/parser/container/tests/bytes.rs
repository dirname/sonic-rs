use bytes::Bytes;

use crate::{
    get, input::JsonSlice, to_array_iter, to_object_iter, to_string, Deserializer, JsonValueTrait,
    OwnedLazyValue, Value,
};

#[test]
fn unvalidated_bytes_are_not_string_storage_and_valid_views_keep_ownership() {
    for input in [b"\"\xff\"".as_slice(), b"{\"\xff\":0}", b"[\"\xff\"]"] {
        let bytes = Bytes::copy_from_slice(input);
        let raw: JsonSlice<'_> = bytes.clone().into();
        assert!(matches!(raw, JsonSlice::Bytes(_)));
        assert_eq!(raw.as_ref(), input);
        assert!(Deserializer::from_json(&bytes)
            .deserialize::<Value>()
            .is_err());
        assert!(Deserializer::from_json(&bytes)
            .deserialize::<OwnedLazyValue>()
            .is_err());
        assert!(to_object_iter(&bytes)
            .next()
            .expect("invalid object")
            .is_err());
        assert!(to_array_iter(&bytes)
            .next()
            .expect("invalid array")
            .is_err());
    }

    let text = "世界 😀".repeat(128);
    let fixture = format!(r#"{{"n":1e-400,"value":["{text}"]}}"#);
    let (dom, owned, raw, cow) = {
        let bytes = Bytes::copy_from_slice(fixture.as_bytes());
        let mut de = Deserializer::from_json(&bytes).use_rawnumber();
        let dom: Value = de.deserialize().expect("byte DOM");
        de.end().expect("complete document");
        let owned: OwnedLazyValue = Deserializer::from_json(&bytes)
            .deserialize()
            .expect("owned lazy bytes");
        let lazy = get(&bytes, ["value"]).expect("selected array");
        let raw = lazy.as_raw_faststr();
        assert!(std::ptr::eq(
            raw.as_bytes().as_ptr(),
            lazy.as_raw_str().as_ptr()
        ));
        assert_eq!(lazy.get(0).as_str(), Some(text.as_str()));
        assert_eq!(lazy.pointer([0]).as_str(), Some(text.as_str()));
        let cow = lazy.as_raw_cow().into_owned();
        let mut iter = lazy.into_array_iter().expect("lazy array iterator");
        assert_eq!(
            iter.next()
                .expect("element")
                .expect("valid element")
                .as_str(),
            Some(text.as_str())
        );
        (dom, owned, raw, cow)
    };
    assert_eq!(to_string(&dom).expect("retained DOM"), fixture);
    assert_eq!(to_string(&owned).expect("retained owned lazy"), fixture);
    assert_eq!(raw.as_str(), cow);
}
