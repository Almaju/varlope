#[derive(uniffi::Enum)]
pub enum YelpSubjectType {
    Service,
    Business,
}

#[derive(uniffi::Record)]
pub struct YelpSuggestion {
    pub title: String,
    pub url: SuggestUrl,
    pub subject_type: YelpSubjectType,
    pub details: YelpDetails, // a uniffi::Record
}
