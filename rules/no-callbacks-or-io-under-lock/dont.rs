#[uniffi::export(with_foreign)]
pub trait MozAdsTelemetry: Send + Sync { fn record_click(&self, placement: String); }

pub struct AdsClient { state: Mutex<State>, telemetry: Arc<dyn MozAdsTelemetry> }

impl AdsClient {
    pub fn record_click(&self, ad: &Ad) -> Result<()> {
        let mut state = self.state.lock();
        state.clicks += 1;
        self.telemetry.record_click(ad.placement.clone()); // flagged: foreign call under the lock
        Request::post(ad.click_url.clone()).send()?; // flagged: network I/O under the lock
        Ok(())
    }
}
