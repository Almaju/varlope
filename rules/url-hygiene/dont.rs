const MARS_BASE_URL: &str = "https://ads.mozilla.org/v1";

fn ads_url(placement_id: &str) -> Result<Url> {
    // flagged: nothing is encoded, and a base ending in `/` gives `//ads`
    let url = format!("{}/ads?placement_id={}", MARS_BASE_URL, placement_id);
    Ok(Url::parse(&url)?)
}

fn click_url() -> Result<Url> {
    Ok(Url::parse(MARS_BASE_URL)?.join("click")?) // flagged: `join` drops `v1`
}
