impl AutofillEngine {
    pub fn reset_local_sync_data(&self) -> Result<()> {
        let db = self.store.lock_db()?;
        let tx = db.unchecked_transaction()?; // flagged: never committed; drop rolls back
        self.storage_impl.reset_storage(&tx)?;
        self.put_meta(&tx, LAST_SYNC_META_KEY, &0)?;
        Ok(())
    }
}
