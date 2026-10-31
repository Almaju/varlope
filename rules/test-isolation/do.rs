impl TestStore {
    pub fn new(client: MockRemoteSettingsClient) -> Self {
        let db_path = open_database::unique_in_memory_db_path();
        Self {
            inner: SuggestStoreInner::new(db_path, vec![], client),
        }
    }
}
