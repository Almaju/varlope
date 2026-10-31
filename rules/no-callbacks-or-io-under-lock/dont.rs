pub fn record_click(&self, click_url: Url) -> Result<()> {
    let mut state = self.state.lock();
    state.click_count += 1;
    // flagged: foreign callback and network I/O while `state` is locked
    self.telemetry.record_client_operation_total("click".into());
    Request::post(click_url).send()?.require_success()?;
    Ok(())
}
