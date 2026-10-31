impl SuggestDao<'_> {
    fn has_yelp_keyword(&self, query: &str) -> Result<bool> {
        let n: i64 = self.conn.query_row( // flagged: COUNT(*) only compared with 0
            "SELECT COUNT(*) FROM yelp_subjects
             WHERE keyword LIKE :query || '%'", // flagged: `%`/`_` in query are wildcards
            named_params! { ":query": query },
            |row| row.get(0),
        )?;
        Ok(n > 0)
    }
}
