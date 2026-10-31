fn is_expired(entry: &CacheEntry, now_ms: u64, ttl_secs: i64) -> bool {
    let ttl_ms = u64::try_from(ttl_secs).unwrap_or(0).saturating_mul(1000);
    now_ms.saturating_sub(entry.fetched_at_ms) > ttl_ms
}
