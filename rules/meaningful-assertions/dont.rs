#[test]
fn test_second_call_hits_cache() {
    // flagged: `.expect(1)` is only checked by `_m.assert()`
    let _m = mock("POST", "/ads").with_body(BODY).expect(1).create();
    let (cache, client) = (make_cache(), make_client());
    let policy = CachePolicy::default();
    cache.send_with_policy(&client, make_request(), &policy).unwrap();
    let (_, outcomes) = cache.send_with_policy(&client, make_request(), &policy).unwrap();
    matches!(outcomes.last(), Some(CacheOutcome::Hit)); // flagged: result discarded
}
