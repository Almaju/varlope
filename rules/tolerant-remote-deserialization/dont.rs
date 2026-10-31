#[derive(Debug, Deserialize)]
pub struct EngineRecord {
    pub identifier: String,
    pub channels: Vec<UpdateChannel>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel { // flagged: no fallback for channels added later
    Release,
    Nightly,
}
