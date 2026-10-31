//! Row mapping for the logins table. // flagged: no MPL-2.0 header above

use rusqlite::Row;

pub(crate) fn guid(row: &Row<'_>) -> rusqlite::Result<String> {
    row.get("guid")
} // flagged: the two blank lines below end the file


