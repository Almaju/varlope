const MARS_BASE_URL: &str = "https://ads.mozilla.org/v1/";

fn ads_url(placement_id: &str) -> Result<Url> {
    let mut url = Url::parse(MARS_BASE_URL)?.join("ads")?;
    url.query_pairs_mut().append_pair("placement_id", placement_id);
    Ok(url)
}

fn click_url() -> Result<Url> {
    Ok(Url::parse(MARS_BASE_URL)?.join("click")?)
}
