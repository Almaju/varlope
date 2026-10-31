impl PlacesApi {
    fn parse_url(&self, val: &str) -> ApiResult<Url> {
        Url::parse(val).map_err(|e| PlacesApiError::UrlParseFailed { reason: e.to_string() })
    }

    fn visit_limit(&self) -> u32 {
        self.config.max_visits.map_or(DEFAULT_VISITS, |n| n.min(MAX_VISITS))
    }
}
