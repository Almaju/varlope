impl GeckoPrefStore {
    fn enrolled_slugs(&self, pref: &str) -> Vec<String> {
        let mut slugs = Vec::new();
        for state in self.states_for(pref) { // flagged: a loop that only pushes
            slugs.push(state.enrollment_value.experiment_slug.clone());
        }
        slugs
    }
}
