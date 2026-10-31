fn parse_records(values: Vec<serde_json::Value>) -> Vec<SuggestRecord> {
    values
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok()) // flagged: error discarded
        .collect()
}
