#[derive(uniffi::Record)]
pub struct YelpResult {
    pub title: String,
    pub url: String, // flagged: use the `Url` custom type
    pub subject_type: i32, // flagged: magic int (0 = service, 1 = business)
    pub details_json: String, // flagged: raw JSON every consumer must parse
}
