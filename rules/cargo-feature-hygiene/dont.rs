impl Client {
    pub fn set_pref_store(
        &self,
        #[cfg(feature = "stateful")] prefs: Arc<GeckoPrefStore>, // flagged
    ) -> Result<()> {
        #[cfg(feature = "stateful")] // flagged: gate the whole fn instead
        self.pref_store.replace(prefs);
        Ok(())
    }
}
