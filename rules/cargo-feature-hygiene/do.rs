impl Client {
    #[cfg(feature = "stateful")]
    pub fn set_pref_store(&self, prefs: Arc<GeckoPrefStore>) -> Result<()> {
        self.pref_store.replace(prefs);
        Ok(())
    }
}
