fn full_keyword(query: &str, keyword: &str) -> Option<String> {
    let rest = keyword.get(query.len()..)?;
    Some(format!("{query}{rest}"))
}
