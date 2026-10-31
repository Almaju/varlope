fn fetch_manifest(&self, url: &Url) -> Result<String> {
    let response = Request::get(url.clone()).send()?;
    let text = response.text().into_owned(); // flagged: body used before status check
    self.cache.write().insert(url.clone(), text.clone());
    Ok(text)
}
