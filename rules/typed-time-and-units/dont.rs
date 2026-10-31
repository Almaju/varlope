#[derive(uniffi::Record)]
pub struct CacheSettings {
    pub max_age_ms: u64, // flagged: use Duration
}

fn last_modified(record: &serde_json::Value) -> SystemTime {
    record["last_modified"].as_u64()
        .map(|ms| UNIX_EPOCH + Duration::from_millis(ms))
        .unwrap_or_else(SystemTime::now) // flagged: bad data silently becomes "now"
}
