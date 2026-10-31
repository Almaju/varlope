impl SuggestDao<'_> {
    fn has_yelp_keyword(&self, query: &str) -> Result<bool> {
        Ok(self.conn.exists(
            "SELECT 1 FROM yelp_subjects
             WHERE keyword BETWEEN :query AND :query || x'FFFF'",
            named_params! { ":query": query },
        )?)
    }
}
