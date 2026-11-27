CREATE TABLE dns_queries (
    id      INTEGER PRIMARY KEY AUTOINCREMENT,
    time    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    proto   TEXT NOT NULL,
    name    TEXT NOT NULL,
    kind    TEXT NOT NULL,
    rcode   TEXT NOT NULL,
    answers INTEGER NOT NULL,
    source  TEXT NOT NULL
);

-- Trạng thái riêng của từng hộp thư. `mailbox` là địa chỉ viết thường; `*` đọc mọi thư; '' là web UI.
CREATE TABLE mailbox_flags (
    mailbox TEXT NOT NULL,
    mail_id INTEGER NOT NULL REFERENCES mails (id) ON DELETE CASCADE,
    flag    TEXT NOT NULL COLLATE NOCASE,
    PRIMARY KEY (mailbox, mail_id, flag)
);

-- Thư đã bị client xoá (EXPUNGE / DELE): chỉ ẩn khỏi hộp đó.
CREATE TABLE mailbox_hidden (
    mailbox TEXT NOT NULL,
    mail_id INTEGER NOT NULL REFERENCES mails (id) ON DELETE CASCADE,
    PRIMARY KEY (mailbox, mail_id)
);

CREATE INDEX mailbox_flags_mail_idx ON mailbox_flags (mail_id);
CREATE INDEX mailbox_hidden_mail_idx ON mailbox_hidden (mail_id);
