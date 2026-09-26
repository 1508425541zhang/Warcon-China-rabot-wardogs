CREATE TABLE integrity_ai_settings (
org_id text PRIMARY KEY REFERENCES organizations(id) ON DELETE CASCADE,
base_url text NOT NULL, model text NOT NULL, key_enc text NOT NULL,
token_parameter text NOT NULL DEFAULT 'max_tokens', max_tokens integer NOT NULL DEFAULT 1200,
updated_at timestamptz NOT NULL DEFAULT now(), last_request_at timestamptz
);
--> statement-breakpoint
CREATE TABLE integrity_ai_reviews (
fingerprint text PRIMARY KEY, case_id text NOT NULL REFERENCES integrity_cases(id) ON DELETE CASCADE,
result jsonb NOT NULL, created_at timestamptz NOT NULL DEFAULT now()
);
