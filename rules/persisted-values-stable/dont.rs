#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, uniffi::Enum)]
#[repr(u8)]
pub enum SuggestionProvider {
    Amp, // flagged: stored as an integer, but the value comes from the order
    Wikipedia,
    Amo,
    Yelp,
}

impl ToSql for SuggestionProvider {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(*self as u8))
    }
}
