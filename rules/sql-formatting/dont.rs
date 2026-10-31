impl SuggestDao<'_> {
    fn fetch_by_keyword(&self, provider: u8, keyword: &str) -> Result<Vec<Suggestion>> {
        self.conn.query_rows_and_then_cached(
            "SELECT s.title, s.url
             FROM suggestions s JOIN keywords k ON k.suggestion_id = s.id
             where s.provider = :provider AND k.keyword = :keyword", // flagged: lowercase `where`
            named_params! { ":provider": provider, ":keyword": keyword },
            |row| self.map_row(row),
        )
    }
}
