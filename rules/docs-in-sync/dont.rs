impl Store {
    /// Deletes all local data, including sync metadata.
    /// Use [`Self::wipe_local`] to keep the metadata. // flagged: unresolved link
    pub fn wipe(&self) -> ApiResult<()> {
        // …
    }

    /// Deletes local records but keeps sync metadata.
    pub fn reset_local_data(&self) -> ApiResult<()> {
        // …
    }
}
