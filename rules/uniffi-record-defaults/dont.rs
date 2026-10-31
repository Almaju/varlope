/// A query for suggestions to show in the address bar.
#[derive(Clone, Debug, Default, uniffi::Record)]
pub struct SuggestionQuery {
    pub keyword: String,
    #[uniffi(default)]
    pub providers: Vec<SuggestionProvider>,
    pub limit: Option<i32>, // flagged: other fields have defaults, this one doesn't
    pub excluded_keywords: Option<Vec<String>>, // flagged: use a Vec; empty means none
    #[uniffi(default = true)]
    pub include_sponsored: bool, // flagged: `Default` gives false
}
