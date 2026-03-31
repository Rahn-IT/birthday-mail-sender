CREATE TABLE sent (
    id INTEGER PRIMARY KEY NOT NULL,
    user_id BLOB NOT NULL REFERENCES people(id),
    sent_at INTEGER NOT NULL
);

CREATE INDEX sent_user_id_idx ON sent(user_id);
CREATE INDEX sent_sent_at_idx ON sent(sent_at);
