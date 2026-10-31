impl GeckoPrefStore {
    fn set_original_values(
        &self,
        states: &[GeckoPrefState],
        store: Option<&GeckoPrefStore>,
        locale: &str,
    ) -> Result<()> {
        let store = store.unwrap_or(self);
        store.write_states(states, locale)
    }
}
