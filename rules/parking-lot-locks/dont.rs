use std::sync::{Arc, Mutex}; // flagged: std mutex

pub struct Store {
    config: Mutex<Option<Arc<Config>>>,
}

impl Store {
    fn config(&self) -> Option<Arc<Config>> {
        self.config.lock().unwrap().clone()
    }
}
