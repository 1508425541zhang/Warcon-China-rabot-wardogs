import { readFileSync, writeFileSync } from 'node:fs';
const p = JSON.parse(readFileSync('package.json', 'utf8'));
p.description =
	'WARDOGS community server management: native Rust backend, SvelteKit UI, PostgreSQL and Docker.';
Object.assign(p.scripts, {
	dev: 'bun run prepare:backend && vite dev',
	build: 'bun run prepare:backend && vite build',
	'prepare:backend': 'bun backend/tools/export-presentation-catalog.ts',
	start: 'node build/index.js',
	preview: 'npm run build && node build/index.js',
	'build:worker':
		'bun run prepare:backend && cargo build --manifest-path backend/Cargo.toml --release --bin warcon-worker',
	worker: 'cargo run --manifest-path backend/Cargo.toml --bin warcon-worker',
	'db:migrate': 'cargo run --manifest-path backend/Cargo.toml --bin migrate -- drizzle',
	'auth:reset': 'cargo run --manifest-path backend/Cargo.toml --bin reset-auth --',
	'backend:dev':
		'bun run prepare:backend && cargo run --manifest-path backend/Cargo.toml --bin warcon-api',
	'backend:test': 'cargo test --manifest-path backend/Cargo.toml --locked -- --include-ignored',
	test: 'bun test src/lib src/routes/page-loads.test.ts'
});
p.engines.node = '>=22.12';
writeFileSync('package.json', JSON.stringify(p, null, '\t') + '\n');
writeFileSync(
	'Dockerfile',
	`# Svelte rendering and transport. All business runs in Dockerfile.native.
FROM node:24-bookworm-slim AS frontend-build
WORKDIR /app
RUN npm install --global bun@1.4.2
COPY package.json bun.lock ./
RUN bun install --frozen-lockfile --ignore-scripts
COPY . .
RUN bun backend/tools/export-presentation-catalog.ts
RUN node node_modules/vite/bin/vite.js build
RUN bun install --frozen-lockfile --production --ignore-scripts

FROM node:24-bookworm-slim
WORKDIR /app
ENV NODE_ENV=production PORT=3000 HOST=0.0.0.0
COPY --from=frontend-build /app/package.json ./package.json
COPY --from=frontend-build /app/build ./build
COPY --from=frontend-build /app/node_modules ./node_modules
USER node
EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=5s --start-period=30s CMD node -e "fetch('http://127.0.0.1:3000/sign-in',{redirect:'manual'}).then(r=>process.exit(r.status<500?0:1)).catch(()=>process.exit(1))"
CMD ["node", "build/index.js"]
`
);
writeFileSync(
	'docker-entrypoint.sh',
	`#!/bin/sh
# Native services have their own image. This entrypoint starts only the Svelte renderer.
set -e
exec node ./build/index.js
`
);
