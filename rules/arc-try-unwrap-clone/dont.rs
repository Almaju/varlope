impl Store {
    fn close(&self) {
        if let Ok(db) = Arc::try_unwrap(self.db.clone()) { // flagged: always fails
            db.into_inner().close();
        }
    }
}
