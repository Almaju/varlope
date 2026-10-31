fn ads_url(&self, placement_id: &str) -> Url {
    let mut url = self.environment.clone().into_url("ads");
    url.query_pairs_mut().append_pair("placement_id", placement_id);
    url
}
