fn ads_url(&self, placement_id: &str) -> Result<Url> {
    let base = "https://ads.mozilla.org/v1"; // flagged: hard-coded endpoint
    // flagged: URL built by string formatting
    let url = format!("{}/ads?placement_id={}", base, placement_id);
    Ok(Url::parse(&url)?)
}
