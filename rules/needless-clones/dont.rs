impl EnrollmentsEvolver<'_> {
    fn conflict_reason(&self, slug: &str, feature_id: &str) -> NotEnrolledReason {
        let store = self.store.clone(); // flagged by clippy: write `Arc::clone(&self.store)`
        let message = format!("{slug} conflicts on {feature_id}").to_string(); // flagged
        store.record_conflict(slug, &message);
        NotEnrolledReason::FeatureConflict { conflict_slug: Some(slug.to_string()) }
    }
}
