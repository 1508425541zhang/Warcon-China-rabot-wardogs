-- Transactional notification input. The trigger copies committed audit input;
-- event classification, access scope, rendering and delivery remain in Rust.
CREATE TABLE webhook_events (
  id text PRIMARY KEY,
  org_id text,
  server_id text,
  kind text NOT NULL,
  payload jsonb NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  processed_at timestamptz
);
CREATE INDEX webhook_events_pending ON webhook_events(created_at) WHERE processed_at IS NULL;
CREATE FUNCTION warcon_queue_audit_notification() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  INSERT INTO webhook_events(id,org_id,server_id,kind,payload)
  VALUES('audit:' || NEW.id,NEW.org_id,NEW.server_id,'audit',to_jsonb(NEW));
  RETURN NEW;
END $$;
CREATE TRIGGER warcon_audit_notification AFTER INSERT ON audit_log
  FOR EACH ROW EXECUTE FUNCTION warcon_queue_audit_notification();
CREATE TABLE webhook_queue (
  id bigserial PRIMARY KEY,
  hook_id text,
  hook_version timestamptz,
  source_id text NOT NULL,
  url_enc text NOT NULL,
  method text NOT NULL DEFAULT 'POST',
  path text NOT NULL DEFAULT '?wait=true',
  payload jsonb,
  state text NOT NULL DEFAULT 'pending',
  attempts integer NOT NULL DEFAULT 0,
  not_before timestamptz NOT NULL DEFAULT now(),
  lease_until timestamptz,
  created_at timestamptz NOT NULL DEFAULT now(),
  done_at timestamptz,
  outcome text NOT NULL DEFAULT '',
  UNIQUE(hook_id,source_id)
);
CREATE INDEX webhook_queue_pending ON webhook_queue(not_before,id) WHERE state='pending';
