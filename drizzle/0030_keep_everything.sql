-- Nothing is deleted. The analytics samples used to be dropped after rawRetentionDays (14) by a
-- TimescaleDB retention policy, and sessions, matches and the hourly rollups after
-- sessionRetentionDays (365) by the worker. Both settings are gone: history is kept for good, and
-- samples are compressed as they age instead, the way kills are (migration 0019): segmented by
-- server, newest first, once a chunk is two weeks old, so the raw reads of the 24-hour and 7-day
-- charts and the hourly rollup never touch a compressed chunk. The retention job lives in the
-- database, so it is removed here; plain Postgres keeps a plain table that grows.
DO $$
BEGIN
	IF EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'timescaledb') THEN
		PERFORM remove_retention_policy('samples', if_exists => TRUE);
		EXECUTE 'ALTER TABLE samples SET (timescaledb.compress, timescaledb.compress_segmentby = ''server_id'', timescaledb.compress_orderby = ''ts DESC'')';
		PERFORM add_compression_policy('samples', INTERVAL '14 days', if_not_exists => TRUE);
	ELSE
		RAISE NOTICE 'timescaledb not installed: samples stays a plain table';
	END IF;
END $$;
