impl SearchEngineUrl {
    fn merge(&mut self, preferred: &JSONEngineUrl) {
        // No `..`: adding a field to JSONEngineUrl fails to compile here.
        let JSONEngineUrl { base, params, is_new_until } = preferred;
        if let Some(base) = base {
            self.base = base.clone();
        }
        if let Some(params) = params {
            self.params = params.clone();
        }
        if let Some(is_new_until) = is_new_until {
            self.is_new_until = Some(is_new_until.clone());
        }
    }
}
