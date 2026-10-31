use parking_lot::Mutex;
use std::sync::Arc;

pub struct Store {
    config: Mutex<Option<Arc<Config>>>,
}

impl Store {
    fn config(&self) -> Option<Arc<Config>> {
        self.config.lock().clone()
    }
}
