fn parse_records(values: Vec<serde_json::Value>) -> Result<Vec<SuggestRecord>> {
    values
        .into_iter()
        .map(|v| Ok(serde_json::from_value(v)?))
        .collect()
}
