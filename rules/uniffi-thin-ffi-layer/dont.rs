#[derive(uniffi::Object)]
pub struct MozAdsClient {
    inner: Mutex<AdsClient>,
    telemetry: Arc<dyn MozAdsTelemetry>, // flagged: the FFI object records telemetry
    worker: BackgroundWorker,            // flagged: the FFI object runs the worker
}

// src/worker.rs, a core module, then needs the FFI types:
// use crate::{AdsClientApiResult, MozAdsClient}; // flagged
