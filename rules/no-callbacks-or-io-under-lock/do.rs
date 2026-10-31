pub fn record_click(&self, click_url: Url) -> Result<()> {
    self.state.lock().click_count += 1; // guard dropped at the end of the statement
    self.telemetry.record_client_operation_total("click".into());
    Request::post(click_url).send()?.require_success()?;
    Ok(())
}
