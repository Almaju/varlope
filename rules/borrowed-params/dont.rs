fn set_original_values(
    &self,
    states: &Vec<GeckoPrefState>, // flagged: ptr_arg
    store: &Option<Arc<GeckoPrefStore>>, // flagged: ref_option
    locale: &String, // flagged: ptr_arg
) -> Result<()> {
    // …
}
