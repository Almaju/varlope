fn record_visit(
    conn: &Connection,
    u: &Url, // flagged: min_ident_chars
    th: VisitType, // flagged: disallowed name `th`
) -> Result<()> {
    let nm = u.as_str(); // flagged: disallowed name `nm`
    conn.execute_cached(
        "INSERT INTO visits (url, visit_type) VALUES (?1, ?2)",
        rusqlite::params![nm, th as u8],
    )?;
    Ok(())
}
