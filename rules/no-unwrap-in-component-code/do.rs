uniffi::custom_type!(JsonValue, String, {
    remote,
    try_lift: |val| Ok(serde_json::from_str(val.as_str())?),
    lower: |obj| obj.to_string(),
});
