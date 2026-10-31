let rows = self.conn.query_rows_and_then_cached(
    // flagged: lowercase keywords; FROM, JOIN and WHERE share a line
    "select s.title, s.url from suggestions s join keywords k on k.suggestion_id = s.id
     where s.provider = :provider and k.keyword = :keyword",
    named_params! { ":provider": provider, ":keyword": keyword },
    |row| self.map_row(row),
)?;
