impl GeckoPrefStore {
    fn enrolled_slugs(&self, pref: &str) -> Vec<String> {
        self.states_for(pref)
            .map(|state| state.enrollment_value.experiment_slug.clone())
            .collect()
    }
}
