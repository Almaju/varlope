fn record_impression(&self, ad: &Ad) -> Result<()> {
    let url = match &ad.callbacks.impression { // flagged: manual_let_else
        Some(url) => url,
        None => return Ok(()),
    };
    // flagged: option_if_let_else, use map_or
    let attempts = if let Some(retries) = self.config.retries { retries + 1 } else { 1 };
    self.send_with_retries(url, attempts)
}
