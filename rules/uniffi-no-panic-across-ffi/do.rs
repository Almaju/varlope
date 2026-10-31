uniffi::custom_type!(JsonValue, String, {
    remote,
    try_lift: |val| Ok(serde_json::from_str(val.as_str())?),
    lower: |obj| obj.to_string(),
});

#[uniffi::export]
impl Store {
    #[uniffi::constructor]
    #[handle_error(Error)]
    pub fn new(path: &str) -> ApiResult<Self> {
        Ok(Self { db: Mutex::new(Db::open(path)?) })
    }
}
