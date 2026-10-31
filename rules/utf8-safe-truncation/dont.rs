const MAX_TITLE_BYTES: usize = 512;

fn truncated_title(tab: &RemoteTabRecord) -> String {
    let mut title = tab.title.clone();
    title.truncate(MAX_TITLE_BYTES); // flagged: panics if byte 512 is inside a char
    title
}
