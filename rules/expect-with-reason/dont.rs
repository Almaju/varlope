impl RequestHash {
    // TODO: remove when cache invalidation is re-enabled behind a Nimbus experiment
    #[allow(dead_code)] // flagged: no reason, and never fails once it is stale
    pub(crate) fn pop_from_url(url: &mut Url) -> Option<Self> {
        // …
    }
}
