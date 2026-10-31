impl EnrollmentsEvolver<'_> {
    fn conflict_reason(&self, slug: &str, feature_id: &str) -> NotEnrolledReason {
        let store = Arc::clone(&self.store);
        let message = format!("{slug} conflicts on {feature_id}");
        store.record_conflict(slug, &message);
        NotEnrolledReason::FeatureConflict { conflict_slug: Some(slug.into()) }
    }
}
