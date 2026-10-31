impl RequestHash {
    // TODO: remove when cache invalidation is re-enabled
    #[allow(dead_code)] // flagged: never fails, even once the code is used or deleted
    pub(crate) fn pop_from_url(url: &mut Url) -> Option<Self> {
        let hash = url.query_pairs().find(|(k, _)| k == "hash")?.1;
        Some(Self(hash.into_owned()))
    }
}
