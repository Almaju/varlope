uniffi::custom_type!(JsonValue, String, {
    remote,
    // flagged: unwrap in try_lift; bad JSON from the caller crashes the app
    try_lift: |val| Ok(serde_json::from_str(val.as_str()).unwrap()),
    lower: |obj| obj.to_string(),
});

#[uniffi::export]
impl Store {
    #[uniffi::constructor]
    pub fn new(path: &str) -> Self {
        Self { db: Mutex::new(Db::open(path).expect("open db")) } // flagged: panics
    }
}
