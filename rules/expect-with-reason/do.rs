impl RequestHash {
    #[expect(
        dead_code,
        reason = "used once cache invalidation is re-enabled behind a Nimbus experiment"
    )]
    pub(crate) fn pop_from_url(url: &mut Url) -> Option<Self> {
        // …
    }
}
