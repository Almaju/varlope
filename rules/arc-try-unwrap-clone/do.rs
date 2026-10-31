impl Store {
    fn close(self) {
        if let Some(db) = Arc::into_inner(self.db) {
            db.into_inner().close();
        }
    }
}
