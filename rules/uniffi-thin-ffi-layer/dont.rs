#[derive(uniffi::Object)]
pub struct MozAdsClient {
    inner: Mutex<AdsClient>,
    worker: BackgroundWorker, // flagged: FFI object owns a worker
}

// src/worker.rs (core)
use crate::ffi::{AdsClientApiResult, MozAdsClient}; // flagged: core imports FFI types

impl BackgroundWorker {
    pub fn run(&self, client: Arc<MozAdsClient>) -> AdsClientApiResult<()> {
        // …
    }
}
