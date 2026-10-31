const MAX_TITLE_BYTES: usize = 512;

pub struct RemoteTab {
    pub title: String,
    pub url_history: Vec<String>,
}

fn trim_title(tab: &mut RemoteTab) {
    tab.title.truncate(MAX_TITLE_BYTES); // flagged: panics if byte 512 is inside an emoji
}
