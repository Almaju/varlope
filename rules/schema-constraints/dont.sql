CREATE TABLE prefix_keywords(
    keyword_prefix TEXT NOT NULL,
    keyword_suffix TEXT NOT NULL DEFAULT '',
    rank INTEGER NOT NULL,
    -- flagged: FK without ON DELETE, and no index starting with suggestion_id
    suggestion_id INTEGER NOT NULL REFERENCES suggestions(id),
    PRIMARY KEY (keyword_prefix, keyword_suffix, suggestion_id)
) WITHOUT ROWID;
