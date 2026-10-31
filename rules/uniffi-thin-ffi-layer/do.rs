#[derive(uniffi::Object)]
pub struct MozAdsClient {
    inner: Arc<Mutex<AdsClient>>, // the core type owns the worker
}

// src/worker.rs (core)
use crate::error::Result;

impl BackgroundWorker {
    pub fn run(&self, client: Arc<Mutex<AdsClient>>) -> Result<()> {
        // …
    }
}
