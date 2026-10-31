#[derive(uniffi::Object)]
pub struct MozAdsClient {
    inner: Mutex<AdsClient>, // the core type owns the worker and telemetry
}

// src/worker.rs, a core module, only knows core types:
// use crate::error::Result;
