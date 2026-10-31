static URL_TEMPLATE: Mutex<String> = Mutex::new(String::new()); // flagged: global state

impl SuggestDao<'_> {
    fn insert_yelp_dataset(&mut self, record: &YelpRecord) -> Result<()> {
        *URL_TEMPLATE.lock() = record.url_template.clone();
        self.insert_yelp_subjects(record)
    }
}
