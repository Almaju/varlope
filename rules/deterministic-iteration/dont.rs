fn engines_in_sync_order(engines: &HashSet<SyncEngineId>) -> Vec<SyncEngineId> {
    let mut order = Vec::new();
    for id in engines { // flagged: iter_over_hash_type, order changes between runs
        order.push(id.clone());
    }
    order
}
