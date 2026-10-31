impl Client {
    #[cfg(feature = "stateful")]
    pub(crate) fn set_pref_store(&self, prefs: Arc<GeckoPrefStore>) -> Result<()> {
        self.pref_store.replace(prefs);
        Ok(())
    }
}

#[cfg(feature = "stateful")]
pub fn init_prefs(client: &Client, prefs: Arc<GeckoPrefStore>) -> Result<()> {
    client.set_pref_store(prefs)
}
