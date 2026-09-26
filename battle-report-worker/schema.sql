CREATE TABLE IF NOT EXISTS reports (
  id TEXT PRIMARY KEY,
  player_id TEXT NOT NULL,
  category TEXT NOT NULL,
  details TEXT NOT NULL,
  reporter_email TEXT,
  status TEXT NOT NULL DEFAULT 'new',
  source TEXT NOT NULL DEFAULT 'web',
  created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_reports_created_at ON reports(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_reports_status ON reports(status);

CREATE TABLE IF NOT EXISTS inbound_emails (
  id TEXT PRIMARY KEY,
  mail_from TEXT NOT NULL,
  rcpt_to TEXT NOT NULL,
  subject TEXT NOT NULL DEFAULT '',
  text_body TEXT NOT NULL DEFAULT '',
  message_id TEXT,
  created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_inbound_emails_created_at ON inbound_emails(created_at DESC);
