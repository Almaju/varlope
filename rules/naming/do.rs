fn record_visit(
    conn: &Connection,
    url: &Url,
    visit_type: VisitType,
) -> Result<()> {
    conn.execute_cached(
        "INSERT INTO visits (url, visit_type) VALUES (?1, ?2)",
        rusqlite::params![url.as_str(), visit_type as u8],
    )?;
    Ok(())
}
