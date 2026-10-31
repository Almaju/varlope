fn fetch_manifest(&self, url: &Url) -> Result<String> {
    let response = Request::get(url.clone()).send()?.require_success()?;
    let text = response.text().into_owned();
    self.cache.write().insert(url.clone(), text.clone());
    Ok(text)
}
