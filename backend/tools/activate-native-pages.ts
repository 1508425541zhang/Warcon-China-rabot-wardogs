/** Mechanical conversion. DTO contracts were snapshotted before activation. */
import { writeFileSync, readdirSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
const root = resolve(import.meta.dir, '../..');
const pages = JSON.parse(
	readFileSync(join(root, 'backend/assets/page-inventory.json'), 'utf8')
) as { source: string; actions: string[] }[];
for (const p of pages) {
	const imports = p.actions.length ? 'pageLoad, pageActions' : 'pageLoad';
	writeFileSync(
		join(root, p.source),
		`import { ${imports} } from '$lib/native/transport.server';\n\nexport const load = (event: Parameters<typeof pageLoad>[0]) => pageLoad(event, ${JSON.stringify(p.source)});\n${p.actions.length ? `export const actions = pageActions(${JSON.stringify(p.source)}, ${JSON.stringify(p.actions)});\n` : ''}`
	);
}
function routes(dir: string): string[] {
	return readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
		e.isDirectory() ? routes(join(dir, e.name)) : e.name === '+server.ts' ? [join(dir, e.name)] : []
	);
}
for (const p of routes(join(root, 'src/routes/api'))) {
	const previous = readFileSync(p, 'utf8');
	const methods = [
		...previous.matchAll(/export const (GET|POST|PUT|PATCH|DELETE|HEAD|OPTIONS)\b/g)
	].map((m) => m[1]);
	if (!methods.length) throw new Error('Missing methods: ' + p);
	writeFileSync(
		p,
		`import { nativeProxy } from '$lib/native/transport.server';\n\n${methods.map((m) => `export const ${m} = nativeProxy;`).join('\n')}\n`
	);
}
for (const p of ['src/routes/metrics/+server.ts', 'src/routes/auth/steam/callback/+server.ts']) {
	writeFileSync(
		join(root, p),
		"import { nativeProxy } from '$lib/native/transport.server';\n\nexport const GET = nativeProxy;\n"
	);
}
writeFileSync(
	join(root, 'src/hooks.server.ts'),
	`import { building } from '$app/environment';
import { redirect, type Handle, type HandleServerError } from '@sveltejs/kit';
import { nativeJson, nativeProxy } from '$lib/native/transport.server';
const HEADERS = {'x-content-type-options':'nosniff','x-frame-options':'DENY','referrer-policy':'same-origin','x-robots-tag':'noindex, nofollow'};
const ACCOUNT_PATH = /^\\/(account|sign-out|join)(\\/|$)/;
export const handle:Handle = async ({event,resolve})=>{
  event.locals.user=null; event.locals.session=null; event.locals.apiKey=null;
  const path=event.url.pathname;
  if (!building && (path.startsWith('/api/') || path==='/metrics' || path==='/auth/steam/callback')) return nativeProxy(event);
  if (!building) {
    const context=await nativeJson<{user:App.Locals['user'];session:App.Locals['session'];gate?:{password:boolean;enrolment:boolean}}>(event,'/api/identity/session');
    event.locals.user=context.user; event.locals.session=context.session;
    if (!ACCOUNT_PATH.test(path)) {
      if (context.gate?.password) redirect(303,'/account?force=1');
      if (context.gate?.enrolment) redirect(303,'/account?enrol=1');
    }
  }
  const response=await resolve(event);
  for(const [key,value] of Object.entries(HEADERS)) response.headers.set(key,value);
  return response;
};
export const handleError:HandleServerError=({status,message})=>({message:status===404?'未找到页面。':message||'服务暂时不可用。'});
`
);
writeFileSync(
	join(root, 'src/routes/sign-out/+server.ts'),
	`import { nativeJson } from '$lib/native/transport.server';
import { redirect, type RequestEvent } from '@sveltejs/kit';
export const POST = async (event:RequestEvent)=>{
  await nativeJson(event,'/api/identity/logout',{}); redirect(303,'/sign-in');
};
export const GET = ()=>redirect(303,'/');
`
);
