fn full_keyword(query: &str, keyword: &str) -> String {
    // flagged: panics when the query is longer than the keyword
    format!("{query}{}", &keyword[query.len()..])
}
