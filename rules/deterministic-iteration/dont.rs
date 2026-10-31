fn engines_to_sync(engines: &HashSet<SyncEngineId>) -> Vec<SyncEngineId> {
    let mut order = Vec::new();
    for id in engines { // flagged: order changes from run to run
        order.push(id.clone());
    }
    order
}
