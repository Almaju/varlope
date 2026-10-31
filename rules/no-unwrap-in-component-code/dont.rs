uniffi::custom_type!(JsonValue, String, {
    remote,
    // flagged: unwrap_used, invalid JSON from the app aborts the browser
    try_lift: |val| Ok(serde_json::from_str(val.as_str()).unwrap()),
    lower: |obj| obj.to_string(),
});
