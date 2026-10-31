/// Records change rarely, so refetch at most once a day to limit the load
/// on the Remote Settings server.
const REFRESH_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

fn needs_refresh(&self, now: SystemTime) -> bool {
    let age = now.duration_since(self.last_fetch).unwrap_or_default();
    age > REFRESH_INTERVAL
}
