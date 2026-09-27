CREATE TABLE personal_plugins (
 user_id text NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
 plugin_id text NOT NULL,
 manifest jsonb NOT NULL,
 server_id text REFERENCES servers(id) ON DELETE SET NULL,
 enabled boolean NOT NULL DEFAULT true,
 updated_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY (user_id, plugin_id)
);
