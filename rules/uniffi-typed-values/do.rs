#[derive(uniffi::Enum)]
pub enum YelpSubjectType {
    Service,
    Business,
}

#[derive(uniffi::Record)]
pub struct YelpResult {
    pub title: String,
    pub url: Url,
    pub subject_type: YelpSubjectType,
    pub details: YelpDetails, // a uniffi::Record
}
