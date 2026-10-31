const URL_TEMPLATE_META_KEY: &str = "yelp_url_template";

impl SuggestDao<'_> {
    fn insert_yelp_dataset(&mut self, record: &YelpRecord) -> Result<()> {
        self.put_meta(URL_TEMPLATE_META_KEY, &record.url_template)?;
        self.insert_yelp_subjects(record)
    }
}
