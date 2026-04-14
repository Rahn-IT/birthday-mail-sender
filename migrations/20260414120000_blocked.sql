CREATE TABLE blocked (
    sha_hash TEXT PRIMARY KEY NOT NULL
);

CREATE INDEX blocked_sha_hash_idx ON blocked(sha_hash);
