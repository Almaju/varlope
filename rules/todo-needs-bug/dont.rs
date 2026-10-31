pub fn sort_titles(titles: &mut [String]) {
    // TODO: switch to the icu4x collator. // flagged: TODO without a bug reference
    titles.sort_by_key(|t| t.to_lowercase());
}
