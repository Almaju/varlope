#[uniffi::export(with_foreign)]
#[async_trait]
pub trait PrimaryPasswordAuthenticator: Send + Sync {
    async fn get_primary_password(&self) -> ApiResult<String>;
    async fn on_authentication_success(&self) -> ApiResult<()>;
}

pub struct Store {
    authenticator: Mutex<Option<Arc<dyn PrimaryPasswordAuthenticator>>>,
}
impl Store {
    pub fn shutdown(&self) { self.authenticator.lock().take(); }
}
