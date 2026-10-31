#[derive(Default)]
pub struct CacheConfig {
    pub ttl_seconds: u64,
    pub max_entries: usize,
}

fn ttl_label(config: &CacheConfig) -> String {
    let ttl = config.ttl_seconds;
    format!("ttl={ttl}s")
}
