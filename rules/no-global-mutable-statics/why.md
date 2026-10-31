Yelp stored its URL template in `static URL_TEMPLATE: Mutex<String>` updated during ingestion, so queries before an ingestion (most of the time, after restart) used an empty template.
