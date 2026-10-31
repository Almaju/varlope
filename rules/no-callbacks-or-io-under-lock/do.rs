#[uniffi::export(with_foreign)]
pub trait MozAdsTelemetry: Send + Sync { fn record_click(&self, placement: String); }

pub struct AdsClient { state: Mutex<State>, telemetry: Arc<dyn MozAdsTelemetry> }

impl AdsClient {
    pub fn record_click(&self, ad: &Ad) -> Result<()> {
        self.state.lock().clicks += 1; // the guard drops at the end of this statement
        self.telemetry.record_click(ad.placement.clone());
        Request::post(ad.click_url.clone()).send()?;
        Ok(())
    }
}
