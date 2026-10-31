impl TestStore {
    pub fn new(client: MockRemoteSettingsClient) -> Self {
        // flagged: every test opens the same shared-cache in-memory DB
        let db_path = "file:test_store_data?mode=memory&cache=shared";
        Self {
            inner: SuggestStoreInner::new(db_path, vec![], client),
        }
    }
}
