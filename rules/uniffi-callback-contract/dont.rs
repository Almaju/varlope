#[uniffi::export(with_foreign)]
#[async_trait]
pub trait PrimaryPasswordAuthenticator: Send + Sync {
    async fn get_primary_password(&self) -> ApiResult<String>;
    fn on_authentication_success(&self); // flagged: no Result, and not async
}

pub struct Store {
    authenticator: Arc<dyn PrimaryPasswordAuthenticator>, // flagged: shutdown can't drop it
}
impl Store {
    pub fn shutdown(&self) {}
}
