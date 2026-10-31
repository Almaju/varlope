/// Provider configs are associated with integer keys stored in the database.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, uniffi::Enum)]
#[repr(u8)]
pub enum SuggestionProvider {
    Amp = 1,
    Wikipedia = 2,
    Amo = 3,
    // 4 is retired: stored values are never reused
    Yelp = 5,
    Mdn = 6,
}
