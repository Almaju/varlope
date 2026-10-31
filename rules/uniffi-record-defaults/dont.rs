/// A query for suggestions to show in the address bar.
#[derive(Clone, Debug, Default, uniffi::Record)]
pub struct SuggestionQuery {
    pub keyword: String,
    pub providers: Vec<SuggestionProvider>,
    pub limit: Option<i32>, // flagged: Option field without a UniFFI default
    pub excluded_keywords: Option<Vec<String>>, // flagged: use a Vec; empty means none
    #[uniffi(default = true)] // flagged: `Default` gives false
    pub include_sponsored: bool,
}
