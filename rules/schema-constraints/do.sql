CREATE TABLE prefix_keywords(
    keyword_prefix TEXT NOT NULL,
    keyword_suffix TEXT NOT NULL DEFAULT '',
    rank INTEGER NOT NULL,
    suggestion_id INTEGER NOT NULL REFERENCES suggestions(id) ON DELETE CASCADE,
    PRIMARY KEY (keyword_prefix, keyword_suffix, suggestion_id)
) WITHOUT ROWID;

CREATE INDEX prefix_keywords_suggestion_id ON prefix_keywords(suggestion_id);
