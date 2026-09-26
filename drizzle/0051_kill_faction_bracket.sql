-- A session snapshot at feed receipt is provisional. Old rows have no verified
-- post-event faction observation and must not enter infantry KPM or baselines.
ALTER TABLE "kills" ADD COLUMN "faction_bracketed" boolean DEFAULT false NOT NULL;
