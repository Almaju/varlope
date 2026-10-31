impl GeckoPrefStore {
    fn set_original_values(
        &self,
        states: &Vec<GeckoPrefState>, // flagged: take `&[GeckoPrefState]`
        store: &Option<Arc<GeckoPrefStore>>, // flagged: take `Option<&Arc<..>>`
        locale: &String, // flagged: take `&str`
    ) -> Result<()> {
        let store = store.as_deref().unwrap_or(self);
        store.write_states(states, locale)
    }
}
