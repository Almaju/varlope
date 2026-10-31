impl PlacesApi {
    fn parse_url(&self, val: &str) -> ApiResult<Url> {
        match Url::parse(val) { // flagged: only converts the error
            Ok(url) => Ok(url),
            Err(e) => Err(PlacesApiError::UrlParseFailed { reason: e.to_string() }),
        }
    }

    fn visit_limit(&self) -> u32 {
        // clippy (`map_unwrap_or`): `.map(..).unwrap_or(..)` is `.map_or(..)`
        self.config.max_visits.map(|n| n.min(MAX_VISITS)).unwrap_or(DEFAULT_VISITS)
    }
}
