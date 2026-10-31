fn has_yelp_keyword(&self, query: &str) -> Result<bool> {
    let n: i64 = self.conn.query_row(
        // flagged: COUNT(*) for existence; LIKE prefix can't use the index
        "SELECT COUNT(*) FROM yelp_subjects WHERE keyword LIKE :query || '%'",
        named_params! { ":query": query },
        |row| row.get(0),
    )?;
    Ok(n > 0)
}
