#[derive(Debug, Deserialize)]
pub struct EngineRecord {
    pub identifier: String,
    pub channels: Vec<UpdateChannel>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    Release,
    Nightly,
    #[serde(other)]
    Unknown, // callers skip records with channels they don't know
}
