impl SearchEngineUrl {
    fn merge(&mut self, preferred: &JSONEngineUrl) { // flagged: never reads `is_new_until`
        if let Some(base) = &preferred.base {
            self.base = base.clone();
        }
        if let Some(params) = &preferred.params {
            self.params = params.clone();
        }
        // `is_new_until`, added to JSONEngineUrl later, is silently never merged
    }
}
