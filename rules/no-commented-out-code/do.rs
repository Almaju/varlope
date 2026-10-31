pub fn fetch_suggestions(&self, query: &SuggestionQuery) -> Result<Vec<Suggestion>> {
    let mut suggestions = self.fetch_from_db(query)?;
    suggestions.sort();
    Ok(suggestions)
}
