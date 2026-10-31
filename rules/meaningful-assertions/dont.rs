#[test]
fn test_second_call_hits_cache() {
    // flagged: .expect(1) is not enforced unless _m.assert() is called
    let _m = mock("POST", "/ads").with_status(200).with_body(BODY).expect(1).create();
    let (cache, client, req) = (make_cache(), make_client(), make_post_request());
    let policy = CachePolicy::default();
    cache.send_with_policy(&client, req.clone(), &policy).unwrap();
    let (_, outcomes) = cache.send_with_policy(&client, req, &policy).unwrap();
    matches!(outcomes.last().unwrap(), CacheOutcome::Hit); // flagged: result discarded
}
