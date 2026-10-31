fn engines_in_sync_order(engines: &BTreeSet<SyncEngineId>) -> Vec<SyncEngineId> {
    let mut order = Vec::new();
    for id in engines { // priority order, from SyncEngineId's derived Ord
        order.push(id.clone());
    }
    order
}
