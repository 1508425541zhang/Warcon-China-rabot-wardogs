-- 'Org lists' (lists.edit) is split in two, one capability per org list: 'Org ban list'
-- (lists.ban) and 'Org reserved slots' (lists.reserve). Every role that held it holds both, and so
-- does every key over the whole org, so nobody's access to the lists changes.
UPDATE "org_roles"
   SET "capabilities" = ("capabilities" - 'lists.edit') || '["lists.ban","lists.reserve"]'::jsonb
 WHERE "capabilities" @> '["lists.edit"]'::jsonb;
--> statement-breakpoint
-- Over the whole org only when the key plainly says so (no server list at all): anything else is
-- read as limited, so a key whose scope is in doubt loses the lists rather than gaining them.
UPDATE "api_keys"
   SET "capabilities" = ("capabilities" - 'lists.edit') || '["lists.ban","lists.reserve"]'::jsonb
 WHERE "capabilities" @> '["lists.edit"]'::jsonb
   AND ("server_ids" IS NULL OR jsonb_typeof("server_ids") = 'null');
--> statement-breakpoint
-- A key limited to some servers was never let into the org lists (they reach every server, and
-- minting such a key has been refused since), so it gets neither: all lists.edit still let it do
-- was save an org-wide Seeding reward and run the lists sync on its servers.
UPDATE "api_keys"
   SET "capabilities" = "capabilities" - 'lists.edit'
 WHERE "capabilities" @> '["lists.edit"]'::jsonb;
