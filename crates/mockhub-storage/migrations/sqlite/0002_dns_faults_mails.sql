ALTER TABLE dns_records ADD COLUMN ttl INTEGER NOT NULL DEFAULT 60;

CREATE TABLE dns_faults (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    name     TEXT NOT NULL,
    kind     TEXT,                       -- NULL = mọi loại record
    action   TEXT NOT NULL,              -- servfail|refused|nxdomain|timeout|delay
    delay_ms INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE mails (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    envelope_from TEXT NOT NULL,
    subject       TEXT NOT NULL,
    text_body     TEXT NOT NULL,
    html_body     TEXT,
    raw           BLOB NOT NULL,
    received_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE mail_recipients (
    id      INTEGER PRIMARY KEY AUTOINCREMENT,
    mail_id INTEGER NOT NULL REFERENCES mails (id) ON DELETE CASCADE,
    kind    TEXT NOT NULL,               -- to|cc|bcc
    address TEXT NOT NULL
);

CREATE INDEX mail_recipients_mail_idx ON mail_recipients (mail_id);
CREATE INDEX mail_recipients_address_idx ON mail_recipients (address COLLATE NOCASE);
