#[uniffi::export]
impl Store {
    #[uniffi::constructor]
    #[handle_error(Error)]
    pub fn new(path: String) -> ApiResult<Self> {
        let db = open_database(&path, &StoreConnectionInitializer)?; // flagged: disk I/O
        Ok(Self { db: Mutex::new(db) })
    }
}
