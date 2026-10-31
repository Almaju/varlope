/// A query for suggestions to show in the address bar.
#[derive(Clone, Debug, Default, uniffi::Record)]
pub struct SuggestionQuery {
    pub keyword: String,
    pub providers: Vec<SuggestionProvider>,
    #[uniffi(default)]
    pub limit: Option<i32>,
    #[uniffi(default)]
    pub excluded_keywords: Vec<String>,
    #[uniffi(default = false)]
    pub include_sponsored: bool,
}
