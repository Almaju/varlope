#[derive(Default)]
pub struct CacheConfig {
    pub ttl_seconds: u64,
    pub max_entries: usize,
}

fn ttl_label(ttl_seconds: u64) -> String {
    format!("ttl={ttl_seconds}s")
}
