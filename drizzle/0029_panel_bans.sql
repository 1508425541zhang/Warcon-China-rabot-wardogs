-- Bans are enforced by the panel now (the worker removes a banned player on sight) and are no
-- longer placed on the game servers, so there is no per-server state to keep for them.
DELETE FROM "server_list_state" WHERE "kind" = 'ban';
