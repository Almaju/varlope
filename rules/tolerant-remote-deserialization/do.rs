#[derive(Debug, Deserialize)]
pub struct EngineRecord {
    pub identifier: String,
    pub is_new_until: Option<String>,
    pub channel: UpdateChannel,
}

#[derive(Debug, Deserialize)]
pub enum UpdateChannel {
    Release,
    Nightly,
    #[serde(other)]
    Unknown, // callers skip records with channels they don't know
}
