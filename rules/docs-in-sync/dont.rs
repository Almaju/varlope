impl Store {
    /// Deletes all local data, including sync metadata.
    /// Use [`Self::wipe_local`] to keep the metadata. // flagged: renamed away
    pub fn wipe(&self) -> ApiResult<()> {
        self.db.lock().wipe()
    }

    /// Deletes local records but keeps sync metadata.
    pub fn reset_local_data(&self) -> ApiResult<()> {
        self.db.lock().reset()
    }
}
