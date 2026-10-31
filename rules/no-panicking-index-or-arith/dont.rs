fn is_expired(entry: &CacheEntry, now_ms: u64, ttl_secs: i64) -> bool {
    let ttl_ms = ttl_secs as u64 * 1000; // flagged: cast_sign_loss, `*` can overflow
    now_ms - entry.fetched_at_ms > ttl_ms // flagged: underflows if the clock went back
}
