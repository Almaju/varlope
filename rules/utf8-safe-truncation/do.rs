const MAX_TITLE_BYTES: usize = 512;

fn truncated_title(tab: &RemoteTabRecord) -> String {
    // cuts at the last char boundary at or below MAX_TITLE_BYTES
    slice_up_to(&tab.title, MAX_TITLE_BYTES).to_owned()
}
