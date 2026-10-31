#[test]
fn mdn_matches_a_keyword_prefix() -> anyhow::Result<()> {
    before_each();
    let store = TestStore::new(MockRemoteSettingsClient::default().with_record(
        SuggestionProvider::Mdn.record("mdn-1", json!([array_mdn()])),
    ));
    store.ingest(SuggestIngestionConstraints::all_providers());
    assert_eq!(store.fetch_suggestions(SuggestionQuery::mdn("array")), vec![array_suggestion()]);
    Ok(())
}
