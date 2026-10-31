#[test]
fn query_mdn_matches_keyword_prefix() -> anyhow::Result<()> {
    before_each();
    let store = TestStore::new(MockRemoteSettingsClient::default().with_record(
        SuggestionProvider::Mdn.record("mdn-1", json!([array_mdn()])),
    ));
    store.ingest(SuggestIngestionConstraints::all_providers());
    let results = store.fetch_suggestions(SuggestionQuery::mdn("array"));
    assert_eq!(results, vec![array_suggestion()], "prefix match");
    Ok(())
}
