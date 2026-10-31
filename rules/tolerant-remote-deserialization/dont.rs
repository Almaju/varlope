#[derive(Debug, Deserialize)]
pub struct EngineRecord {
    pub identifier: String,
    pub is_new_until: String, // flagged: new field without Option or #[serde(default)]
    pub channel: UpdateChannel,
}

#[derive(Debug, Deserialize)]
pub enum UpdateChannel { // flagged: no #[serde(other)] fallback variant
    Release,
    Nightly,
}
