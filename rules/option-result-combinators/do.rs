fn record_impression(&self, ad: &Ad) -> Result<()> {
    let Some(url) = &ad.callbacks.impression else {
        return Ok(());
    };
    let attempts = self.config.retries.map_or(1, |retries| retries + 1);
    self.send_with_retries(url, attempts)
}
