const MAX_TITLE_BYTES: usize = 512;

pub struct RemoteTab {
    pub title: String,
    pub url_history: Vec<String>,
}

fn trim_title(tab: &mut RemoteTab) {
    // cuts at the last char boundary at or below MAX_TITLE_BYTES
    tab.title = slice_up_to(&tab.title, MAX_TITLE_BYTES).to_owned();
}
