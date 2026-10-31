#[test]
fn query() -> anyhow::Result<()> { // flagged in review: catch-all test keeps growing
    // … 300 lines of AMP, Wikipedia, AMO and Yelp cases …
    let store = TestStore::new(MockRemoteSettingsClient::default().with_record(
        SuggestionProvider::Mdn.record("mdn-1", json!([array_mdn()])),
    ));
    store.ingest(SuggestIngestionConstraints::all_providers());
    let results = store.fetch_suggestions(SuggestionQuery::mdn("array"));
    assert_eq!(results, vec![array_suggestion()]);
    Ok(())
}
