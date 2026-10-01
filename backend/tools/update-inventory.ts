import { readFileSync, writeFileSync, readdirSync } from 'node:fs';
import { join, relative } from 'node:path';
const root = join(import.meta.dir, '../..');
const file = join(root, 'docs/rust-route-inventory.json');
const inventory = JSON.parse(readFileSync(file, 'utf8'));
const implemented = new Set([
    'GET /api/servers/[id]/kills', 'GET /api/servers/[id]/matches',
    'GET /api/servers/[id]/matches/[matchId]',
    'POST /api/servers/[id]/players/[steamId]/notes',
    'DELETE /api/servers/[id]/players/[steamId]/notes/[noteId]',
    'PUT /api/servers/[id]/players/[steamId]/watch',
    'GET /api/orgs/[id]/roles', 'POST /api/orgs/[id]/roles',
    'PATCH /api/orgs/[id]/roles/[roleId]', 'DELETE /api/orgs/[id]/roles/[roleId]',
    'POST /api/orgs/[id]/roles/[roleId]/reset',
    'GET /api/orgs/[id]/keys', 'POST /api/orgs/[id]/keys', 'DELETE /api/orgs/[id]/keys/[keyId]',
    'GET /api/servers/[id]/outbox', 'POST /api/servers/[id]/numeric-limits',
    'POST /api/servers/[id]/faction-lock', 'POST /api/servers/[id]/skill-balance',
    'POST /api/servers/[id]/weapon-restrictions', 'GET /api/servers/[id]/feed',
    'POST /api/servers/[id]/feed', 'DELETE /api/servers/[id]/feed',
    'GET /api/servers/[id]/analytics', 'GET /api/servers/[id]/cash',
    'GET /api/settings', 'PUT /api/settings'
]);
for (const route of inventory.routes) {
    const key = `${route.method} ${route.path}`;
    if (implemented.has(key)) { route.status = 'implemented'; route.verification = 'local PostgreSQL / HTTP contract suite'; }
    else if (key === 'POST /api/ingest/events') { route.status = 'partial'; route.verification = 'atomic raw/kills/dual-job writes tested; SSE and full consumers pending'; }
    else if (key === 'GET /api/health') { route.status = 'partial'; route.verification = 'public response implemented; worker/owner diagnostics pending'; }
}
const files: string[] = [];
function walk(folder: string) {
    for (const entry of readdirSync(folder, { withFileTypes: true })) {
        const path = join(folder, entry.name);
        if (entry.isDirectory()) walk(path);
        else if (entry.name === '+page.server.ts' || entry.name === '+layout.server.ts') files.push(path);
    }
}
walk(join(root, 'src/routes'));
inventory.pageServerEntrypoints = files.sort().map(path => ({
    source: relative(root, path).replaceAll('\\', '/'),
    entrypoints: ['load', 'actions'].filter(name => new RegExp(`export\\s+(?:const|async\\s+function|function)\\s+${name}\\b`).test(readFileSync(path, 'utf8'))),
    status: 'pending'
}));
inventory.backgroundTasks = [
    ['worker ownership/fencing', 'src/lib/server/leadership.ts', 'implemented'],
    ['Steam profile refresh', 'src/lib/server/integrity/profile-refresh.ts', 'implemented'],
    ['feed ordered claims/acknowledgements', 'src/lib/server/feed-processing.ts', 'partial'],
    ['poller/observation/automation', 'src/lib/server/poller.ts', 'pending'],
    ['delivery/outbox', 'src/lib/server/outbox.ts', 'pending'],
    ['Integrity evaluation', 'src/lib/server/integrity/pipeline.ts', 'pending'],
    ['baseline rebuild', 'src/lib/server/integrity/baselines.ts', 'pending'],
    ['Steam playtime', 'src/lib/server/steam-playtime.ts', 'implemented'],
    ['sample hourly rollups', 'src/lib/server/rollups.ts', 'implemented'],
    ['AI review queue', 'src/lib/server/integrity/ai-queue.ts', 'pending'],
    ['long model queue', 'src/lib/server/integrity/model-runtime.ts', 'pending'],
    ['expanded30m native inference / source reader / HTTP service', 'services/integrity-model-bun/server.ts', 'implemented'],
    ['short model', 'src/lib/server/integrity/short-risk.ts', 'pending'],
    ['history retention', 'src/lib/server/integrity/history-retention.ts', 'pending'],
    ['QQ runtime', 'src/lib/server/qq/runtime.ts', 'pending'],
    ['webhook delivery', 'src/lib/server/webhook-delivery.ts', 'pending'],
    ['worker relay/SSE', 'src/worker/runtime.ts', 'pending'],
    ['settings reload', 'src/lib/server/settings.ts', 'partial']
].map(([name, source, status]) => ({ name, source, status }));
inventory.scope = 'All business APIs, page server business loaders/actions and background tasks. Svelte UI stays.';
inventory.activation = 'local development only; not connected to production or Svelte request routing';
writeFileSync(file, JSON.stringify(inventory, null, 2) + '\n');
console.log(JSON.stringify({ operations: inventory.routes.length, implemented: inventory.routes.filter((r: any) => r.status === 'implemented').length, pageServerFiles: files.length }));
