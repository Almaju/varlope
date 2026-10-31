/// Provider configs are associated with integer keys stored in the database.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, uniffi::Enum)]
#[repr(u8)]
pub enum SuggestionProvider { // flagged: persisted as integers, implicit values
    Amp,
    Wikipedia,
    Amo,
    Yelp,
    Mdn,
}
