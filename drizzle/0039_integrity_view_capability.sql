-- Custom SQL migration file, put your code below! --
-- Built-in admin roles gain the new read capability. Custom roles keep their existing grants.
UPDATE "org_roles"
   SET "capabilities" = "capabilities" || '["integrity.view"]'::jsonb,
       "updated_at" = now()
 WHERE "builtin" = 'admin'
   AND NOT "capabilities" @> '["integrity.view"]'::jsonb;
