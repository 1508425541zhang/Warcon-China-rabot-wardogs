ALTER TABLE integrity_ai_settings ADD COLUMN auto_enabled boolean NOT NULL DEFAULT true;
ALTER TABLE integrity_ai_settings ADD COLUMN daily_limit integer NOT NULL DEFAULT 100;
ALTER TABLE integrity_ai_settings ADD COLUMN budget_day text NOT NULL DEFAULT '';
ALTER TABLE integrity_ai_settings ADD COLUMN daily_requests integer NOT NULL DEFAULT 0;
--> statement-breakpoint
CREATE TABLE integrity_ai_jobs (
case_id text PRIMARY KEY REFERENCES integrity_cases(id) ON DELETE CASCADE,
state text NOT NULL DEFAULT 'pending', attempts integer NOT NULL DEFAULT 0,
next_at timestamptz NOT NULL DEFAULT now(), lease_until timestamptz, claim_token text,
last_error text, result jsonb, updated_at timestamptz NOT NULL DEFAULT now());
--> statement-breakpoint
CREATE INDEX integrity_ai_jobs_due_idx ON integrity_ai_jobs(state,next_at);
