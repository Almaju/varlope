#[derive(uniffi::Record)]
pub struct CacheSettings {
    pub max_age: Duration,
}

fn last_modified(record: &serde_json::Value) -> Result<SystemTime> {
    let ms = record["last_modified"].as_u64().ok_or(Error::InvalidTimestamp)?;
    Ok(UNIX_EPOCH + Duration::from_millis(ms))
}
