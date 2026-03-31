CREATE TABLE people (
    id BLOB PRIMARY KEY NOT NULL,
    first_name TEXT NOT NULL,
    last_name TEXT NOT NULL,
    greeting TEXT NOT NULL,
    email TEXT NOT NULL,
    birthday TEXT NOT NULL,
    start_year INTEGER NOT NULL
);

CREATE INDEX people_last_name_idx ON people(last_name, first_name);
