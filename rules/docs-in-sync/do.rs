impl Store {
    /// Deletes all local data, including sync metadata.
    /// Use [`Self::reset_local_data`] to keep the metadata.
    pub fn wipe(&self) -> ApiResult<()> {
        // …
    }

    /// Deletes local records but keeps sync metadata.
    pub fn reset_local_data(&self) -> ApiResult<()> {
        // …
    }
}
