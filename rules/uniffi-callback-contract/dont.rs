#[uniffi::export(with_foreign)]
#[async_trait]
pub trait PrimaryPasswordAuthenticator: Send + Sync {
    async fn get_primary_password(&self) -> ApiResult<String>;
    fn on_authentication_success(&self); // flagged: sync, and no Result
}

pub struct Store {
    authenticator: Arc<dyn PrimaryPasswordAuthenticator>,
}
impl Store {
    pub fn shutdown(&self) {} // flagged: foreign callback outlives shutdown
}
