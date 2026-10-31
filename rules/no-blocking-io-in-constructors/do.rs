#[uniffi::export]
impl Store {
    #[uniffi::constructor]
    pub fn new(path: String) -> Self {
        let flags = OpenFlags::default();
        // the DB is opened on first use, by a method the app calls in the background
        Self { db: LazyDb::new(Path::new(&path), flags, StoreConnectionInitializer) }
    }
}
