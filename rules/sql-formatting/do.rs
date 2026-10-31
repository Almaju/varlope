let rows = self.conn.query_rows_and_then_cached(
    r#"
    SELECT s.title, s.url
    FROM suggestions s
    JOIN keywords k ON k.suggestion_id = s.id
    WHERE s.provider = :provider
      AND k.keyword = :keyword
    "#,
    named_params! { ":provider": provider, ":keyword": keyword },
    |row| self.map_row(row),
)?;
