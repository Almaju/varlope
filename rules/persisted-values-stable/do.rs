#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, uniffi::Enum)]
#[repr(u8)]
pub enum SuggestionProvider {
    Amp = 1,
    Wikipedia = 2,
    // 3 was Pocket: retired values are never reused.
    Yelp = 5,
}

impl ToSql for SuggestionProvider {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(*self as u8))
    }
}
