impl Client {
    pub(crate) fn set_pref_store( // flagged: its only caller is stateful-only
        &self,
        #[cfg(feature = "stateful")] prefs: Arc<GeckoPrefStore>,
    ) -> Result<()> {
        #[cfg(feature = "stateful")]
        self.pref_store.replace(prefs);
        Ok(())
    }
}

#[cfg(feature = "stateful")]
pub fn init_prefs(client: &Client, prefs: Arc<GeckoPrefStore>) -> Result<()> {
    client.set_pref_store(prefs)
}
