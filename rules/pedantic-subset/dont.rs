pub struct CacheConfig {
    pub ttl_seconds: u64,
    pub max_entries: usize,
}

impl Default for CacheConfig { // flagged: `derivable_impls`
    fn default() -> Self {
        Self { ttl_seconds: 0, max_entries: 0 }
    }
}

fn ttl_label(config: &CacheConfig) -> String {
    let ttl = config.ttl_seconds;
    format!("ttl={}s", ttl) // flagged: `uninlined_format_args`
}
