impl SuggestStore {
    pub fn fetch_suggestions(&self, query: &SuggestionQuery) -> Result<Vec<Suggestion>> {
        let mut suggestions = self.fetch_from_db(query)?;
        // suggestions.retain(|s| !s.is_sponsored()); // flagged: commented-out code
        // let limit = query.limit.unwrap_or(10);
        suggestions.sort();
        Ok(suggestions)
    }
}
