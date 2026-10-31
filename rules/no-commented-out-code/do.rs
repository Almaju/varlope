impl SuggestStore {
    pub fn fetch_suggestions(&self, query: &SuggestionQuery) -> Result<Vec<Suggestion>> {
        let mut suggestions = self.fetch_from_db(query)?;
        // Sponsored suggestions stay in: the caller decides what to show.
        suggestions.sort();
        Ok(suggestions)
    }
}
