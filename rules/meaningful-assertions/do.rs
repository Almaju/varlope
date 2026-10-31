#[test]
fn test_second_call_hits_cache() {
    let m = mock("POST", "/ads").with_body(BODY).expect(1).create();
    let (cache, client) = (make_cache(), make_client());
    let policy = CachePolicy::default();
    cache.send_with_policy(&client, make_request(), &policy).unwrap();
    let (_, outcomes) = cache.send_with_policy(&client, make_request(), &policy).unwrap();
    assert!(matches!(outcomes.last(), Some(CacheOutcome::Hit)));
    m.assert();
}
