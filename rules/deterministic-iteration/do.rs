fn engines_to_sync(engines: &BTreeSet<SyncEngineId>) -> Vec<SyncEngineId> {
    let mut order = Vec::new();
    for id in engines { // SyncEngineId's derived `Ord` gives a stable order
        order.push(id.clone());
    }
    order
}
