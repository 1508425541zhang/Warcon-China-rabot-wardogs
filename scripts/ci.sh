#!/usr/bin/env bash
# What CI runs (.github/workflows/ci.yml), step for step, on this machine: install, lint, check,
# the test suites against a database, the production build and the smoke test of that build, and
# with --docker the image build too. A green run here is a green push; the first failing step is
# named the way CI would fail on it.
#
# It needs a Postgres with TimescaleDB to make throwaway databases on: the container the tests
# already use (README, "Local development"), the same image as CI's service:
#
#   docker run -d --name warcon-pg-test -p 5434:5432 -e POSTGRES_USER=warcon \
#     -e POSTGRES_PASSWORD=warcon -e POSTGRES_DB=warcon timescale/timescaledb:2.30.0-pg18
#
# CI_DB names another server (postgres://user:pass@host:port, no database on the end). Nothing
# from .env is read, since CI has none: a Steam key or a dev DATABASE_URL there would change what
# the smoke test sees. Port 5199 must be free, as it is on CI.
set -uo pipefail
cd "$(dirname "$0")/.."
DB=${CI_DB:-postgres://warcon:warcon@127.0.0.1:5434}
SMOKE_DB=warcon_ci_smoke
PORT=5199
ORIGIN=http://127.0.0.1:$PORT
LOG=$(mktemp -t warcon-ci-server)
SMOKE_OUT=$(mktemp -t warcon-ci-smoke)
SERVER_PID=

step() { printf '\n\033[1m== %s\033[0m\n' "$1"; }
fail() { printf '\n\033[31mCI would fail at: %s\033[0m\n' "$1"; exit 1; }
# One statement on the server's maintenance database, through Bun's own client (no psql needed).
sqladmin() {
	ADMIN_URL="$DB/postgres" STMT="$1" bun --no-env-file -e '
		const { SQL } = require("bun");
		(async () => {
			const sql = new SQL(process.env.ADMIN_URL, { max: 1 });
			await sql.unsafe(process.env.STMT);
			await sql.close();
		})().catch((err) => { console.error(err.message); process.exit(1); });'
}
cleanup() {
	if [ -n "$SERVER_PID" ] && kill -0 "$SERVER_PID" 2>/dev/null; then
		kill "$SERVER_PID" 2>/dev/null
		wait "$SERVER_PID" 2>/dev/null
	fi
	sqladmin "DROP DATABASE IF EXISTS \"$SMOKE_DB\"" >/dev/null 2>&1 || true
	rm -f "$SMOKE_OUT"
}
trap cleanup EXIT

sqladmin 'SELECT 1' >/dev/null 2>&1 || fail "no Postgres at $DB (start the warcon-pg-test container, or set CI_DB)"

step 'bun install --frozen-lockfile'
bun install --frozen-lockfile || fail 'install'

step 'bun run lint'
bun run lint || fail 'lint (prettier --check .)'

step 'bun run check'
bun run check || fail 'check (svelte-check)'

step 'bun test'
DATABASE_URL="$DB/$SMOKE_DB" TEST_DATABASE_URL="$DB/postgres" bun --no-env-file test || fail 'bun test'

step 'bun run build'
bun run build || fail 'build'

step 'Smoke test the production build'
if lsof -ti:"$PORT" >/dev/null 2>&1; then fail "port $PORT is in use here and free on CI; stop what listens on it"; fi
sqladmin "DROP DATABASE IF EXISTS \"$SMOKE_DB\"" && sqladmin "CREATE DATABASE \"$SMOKE_DB\"" || fail "could not make $SMOKE_DB on $DB"
BETTER_AUTH_SECRET=$(openssl rand -base64 32) ENCRYPTION_KEY=$(openssl rand -base64 32) \
	DATABASE_URL="$DB/$SMOKE_DB" ORIGIN=$ORIGIN PORT=$PORT HOST=127.0.0.1 POLL_SECONDS=5 \
	bun --no-env-file ./build/index.js >"$LOG" 2>&1 &
SERVER_PID=$!
up=0
for _ in $(seq 1 60); do
	if curl -sf "$ORIGIN/api/health" >/dev/null; then up=1; break; fi
	sleep 1
done
if [ "$up" != 1 ]; then tail -30 "$LOG"; fail 'server did not become healthy within 60s'; fi
WARCON_URL=$ORIGIN bash scripts/smoke.sh | tee "$SMOKE_OUT"
if ! grep -q 'failed=0' "$SMOKE_OUT"; then
	printf '\n== server log (tail)\n'
	tail -40 "$LOG"
	fail 'smoke test'
fi

if [ "${1:-}" = "--docker" ]; then
	step 'docker build'
	docker build -t warcon-ci . || fail 'docker build'
fi

printf '\n\033[32mCI would pass.\033[0m\n'
