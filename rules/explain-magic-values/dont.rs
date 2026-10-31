fn needs_refresh(&self, now: SystemTime) -> bool {
    let age = now.duration_since(self.last_fetch).unwrap_or_default();
    age > Duration::from_secs(24 * 60 * 60) // flagged: why 24 hours?
}
