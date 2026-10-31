// `query()` already covers AMP, Wikipedia, AMO and Yelp; flagged in review as
// "getting a little unwieldy".

#[test]
fn test_basic() -> anyhow::Result<()> { // flagged: the name says nothing
    before_each();
    let store = TestStore::new(MockRemoteSettingsClient::default().with_record(
        SuggestionProvider::Mdn.record("mdn-1", json!([array_mdn()])),
    ));
    store.ingest(SuggestIngestionConstraints::all_providers());
    assert_eq!(store.fetch_suggestions(SuggestionQuery::mdn("array")), vec![array_suggestion()]);
    Ok(())
}
