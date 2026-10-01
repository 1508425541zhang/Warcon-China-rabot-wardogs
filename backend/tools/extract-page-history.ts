// Preserve the existing three-source history projection while replacing Drizzle parameters.
const source = await Bun.file(
	new URL('../../src/lib/server/integrity/action-history.ts', import.meta.url)
).text();
let sql = source.match(
	/>\(sql`(WITH history AS \(\$\{historyIndex\(serverId\)\}[\s\S]*?)`\);/
)?.[1];
if (!sql) throw new Error('History query not found');
const index = source.match(/return sql`([\s\S]*?)`;/)?.[1];
if (!index) throw new Error('History index not found');
const history = index.replaceAll('${serverId}', '$1');
sql = sql
	.replaceAll('${historyIndex(serverId)}', history)
	.replaceAll(
		'${scope}',
		"created_at<=$2 AND ($3='all' OR ($3='manual' AND source='REVIEW') OR ($3='automatic' AND source<>'REVIEW'))"
	)
	.replaceAll('${pageSize}', '$4')
	.replaceAll('${(page - 1) * pageSize}', '$5')
	.replaceAll('${serverId}', '$1')
	.replaceAll('${SHORT_WARNING}', '$6::float8')
	.replaceAll('${SHORT_KICK}', '$7::float8');
if (sql.includes('${')) throw new Error('Unresolved SQL binding');
await Bun.write(
	new URL('../sql/page-history.sql', import.meta.url),
	'SELECT to_jsonb(rows) FROM (' + sql + ') rows\n'
);
await Bun.write(
	new URL('../sql/page-history-count.sql', import.meta.url),
	`WITH history AS (${history}) SELECT count(*)::bigint FROM history WHERE created_at<=$2 AND ($3='all' OR ($3='manual' AND source='REVIEW') OR ($3='automatic' AND source<>'REVIEW'))\n`
);
