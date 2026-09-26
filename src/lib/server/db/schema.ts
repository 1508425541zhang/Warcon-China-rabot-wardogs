// The whole database, as Drizzle tables. drizzle-kit reads this file to generate migrations
// (bun run db:generate); the app applies them at startup. Keep it free of SvelteKit imports.
import { sql } from 'drizzle-orm';
import {
	bigint,
	bigserial,
	boolean,
	check,
	customType,
	index,
	integer,
	pgTable,
	primaryKey,
	real,
	text,
	timestamp,
	uniqueIndex
} from 'drizzle-orm/pg-core';

const ts = (name: string) => timestamp(name, { withTimezone: true, mode: 'date' });

/**
 * jsonb that hands the value to Bun's SQL driver as is. Drizzle's own jsonb() stringifies first
 * and Bun then JSON-encodes that string again, so arrays and objects landed in Postgres as JSON
 * *strings* (jsonb_typeof = 'string'): fine to read back through Drizzle, unusable inside SQL.
 * Migration 0008 repairs rows written that way. Reads still accept the old shape.
 */
const jsonb = customType<{ data: unknown; driverData: unknown }>({
	dataType: () => 'jsonb',
	toDriver: (value) => value,
	fromDriver: (value) => {
		if (typeof value !== 'string') return value;
		try {
			return JSON.parse(value);
		} catch {
			return value;
		}
	}
});

// ---- Better Auth (core + username + admin plugins, plus Warcon's mustChangePassword) -------------

export const user = pgTable('user', {
	id: text('id').primaryKey(),
	name: text('name').notNull(),
	email: text('email').notNull().unique(),
	emailVerified: boolean('email_verified').notNull().default(false),
	image: text('image'),
	createdAt: ts('created_at').notNull().defaultNow(),
	updatedAt: ts('updated_at').notNull().defaultNow(),
	// username plugin
	username: text('username').unique(),
	displayUsername: text('display_username'),
	// admin plugin: role is "owner" | "member"; banned doubles as "disabled"
	role: text('role'),
	banned: boolean('banned').default(false),
	banReason: text('ban_reason'),
	banExpires: ts('ban_expires'),
	// warcon
	mustChangePassword: boolean('must_change_password').notNull().default(false),
	/** the member's own SteamID64, so an org can hand its members a reserved slot */
	steamId: text('steam_id').unique(),
	/** the organisation the panel opens on (dashboard, switcher, Servers); null = every org */
	defaultOrgId: text('default_org_id').references(() => organizations.id, { onDelete: 'set null' }),
	// two-factor plugin
	twoFactorEnabled: boolean('two_factor_enabled').default(false),
	// warcon sign-in policy (see enrolment.ts): recomputed whenever a sign-in method changes
	/** the account meets the sign-in rules (two ways in, a second factor on the password, ...) */
	authComplete: boolean('auth_complete').notNull().default(false),
	/** first sign-in since the rules arrived; the grace period counts from here */
	authGraceStartedAt: ts('auth_grace_started_at'),
	/** sha-256 of the one-time recovery key; null = none issued (or the last one was used) */
	recoveryKeyHash: text('recovery_key_hash'),
	recoveryKeyAt: ts('recovery_key_at')
});

export const session = pgTable(
	'session',
	{
		id: text('id').primaryKey(),
		expiresAt: ts('expires_at').notNull(),
		token: text('token').notNull().unique(),
		createdAt: ts('created_at').notNull().defaultNow(),
		updatedAt: ts('updated_at').notNull().defaultNow(),
		ipAddress: text('ip_address'),
		userAgent: text('user_agent'),
		userId: text('user_id')
			.notNull()
			.references(() => user.id, { onDelete: 'cascade' }),
		impersonatedBy: text('impersonated_by')
	},
	(t) => [index('session_user_id_idx').on(t.userId)]
);

export const account = pgTable(
	'account',
	{
		id: text('id').primaryKey(),
		accountId: text('account_id').notNull(),
		providerId: text('provider_id').notNull(),
		userId: text('user_id')
			.notNull()
			.references(() => user.id, { onDelete: 'cascade' }),
		accessToken: text('access_token'),
		refreshToken: text('refresh_token'),
		idToken: text('id_token'),
		accessTokenExpiresAt: ts('access_token_expires_at'),
		refreshTokenExpiresAt: ts('refresh_token_expires_at'),
		scope: text('scope'),
		password: text('password'),
		createdAt: ts('created_at').notNull().defaultNow(),
		updatedAt: ts('updated_at').notNull().defaultNow(),
		issuer: text('issuer').notNull().default('')
	},
	(t) => [
		index('account_user_id_idx').on(t.userId),
		uniqueIndex('account_issuer_account_id_uidx').on(t.issuer, t.accountId)
	]
);

export const verification = pgTable(
	'verification',
	{
		id: text('id').primaryKey(),
		identifier: text('identifier').notNull(),
		value: text('value').notNull(),
		expiresAt: ts('expires_at').notNull(),
		createdAt: ts('created_at').notNull().defaultNow(),
		updatedAt: ts('updated_at').notNull().defaultNow()
	},
	(t) => [index('verification_identifier_idx').on(t.identifier)]
);

/** two-factor plugin: one TOTP secret and the (encrypted) backup codes per user */
export const twoFactor = pgTable(
	'two_factor',
	{
		id: text('id').primaryKey(),
		secret: text('secret').notNull(),
		backupCodes: text('backup_codes').notNull(),
		userId: text('user_id')
			.notNull()
			.references(() => user.id, { onDelete: 'cascade' }),
		/** false between "enable" and the first code the user proves they can produce */
		verified: boolean('verified').default(true),
		failedVerificationCount: integer('failed_verification_count').default(0),
		lockedUntil: ts('locked_until')
	},
	(t) => [index('two_factor_user_id_idx').on(t.userId), index('two_factor_secret_idx').on(t.secret)]
);

/** passkey plugin: WebAuthn credentials; a user may hold several (phone, laptop, security key) */
export const passkey = pgTable(
	'passkey',
	{
		id: text('id').primaryKey(),
		name: text('name'),
		publicKey: text('public_key').notNull(),
		userId: text('user_id')
			.notNull()
			.references(() => user.id, { onDelete: 'cascade' }),
		credentialID: text('credential_id').notNull(),
		counter: integer('counter').notNull(),
		deviceType: text('device_type').notNull(),
		backedUp: boolean('backed_up').notNull(),
		transports: text('transports'),
		createdAt: ts('created_at'),
		aaguid: text('aaguid')
	},
	(t) => [
		index('passkey_user_id_idx').on(t.userId),
		index('passkey_credential_id_idx').on(t.credentialID)
	]
);

// ---- Warcon ------------------------------------------------------------------------------------

/** A clan / community. Servers belong to exactly one org; people join through invite links. */
export const organizations = pgTable('organizations', {
	id: text('id').primaryKey(),
	name: text('name').notNull(),
	slug: text('slug').notNull().unique(),
	createdBy: text('created_by'),
	/** site-owner override of MAX_SERVERS_PER_ORG; null = the instance default */
	serverLimit: integer('server_limit'),
	/** set by the site owner: members lose access, nothing can be added or joined until cleared */
	suspendedAt: ts('suspended_at'),
	suspendedReason: text('suspended_reason').notNull().default(''),
	/** members who set a SteamID on their account get a reserved slot on every org server */
	membersReserved: boolean('members_reserved').notNull().default(false),
	/**
	 * Site-owner allowances: what this org's owners may switch on per server, allowed unless the
	 * site owner withdraws it. Each public surface needs the allowance and the server's own
	 * switch; $lib/features computes the effective set.
	 */
	allowPublicStatus: boolean('allow_public_status').notNull().default(true),
	allowPublicLeaderboards: boolean('allow_public_leaderboards').notNull().default(true),
	/** a discord.gg or discord.com/invite link, shown as a button on the org's public pages; '' = none */
	discordInviteUrl: text('discord_invite_url').notNull().default(''),
	/** what a banned player is shown: the reason and facts about the ban, see $lib/ban-message */
	banMessage: text('ban_message').notNull().default('{reason}'),
	createdAt: ts('created_at').notNull().defaultNow(),
	updatedAt: ts('updated_at').notNull().defaultNow()
});

/**
 * An org's server roles: a name and the capabilities it carries (see $lib/capabilities). Every org
 * starts with the three built-ins, which owners may edit but not delete; custom roles are more rows.
 */
export const orgRoles = pgTable(
	'org_roles',
	{
		id: text('id').primaryKey(),
		orgId: text('org_id')
			.notNull()
			.references(() => organizations.id, { onDelete: 'cascade' }),
		name: text('name').notNull(),
		/** Capability[] */
		capabilities: jsonb('capabilities').notNull(),
		/** which built-in this row started as; null for custom roles. Built-ins can be reset. */
		builtin: text('builtin', { enum: ['viewer', 'operator', 'admin'] }),
		sortOrder: integer('sort_order').notNull().default(0),
		createdAt: ts('created_at').notNull().defaultNow(),
		updatedAt: ts('updated_at').notNull().defaultNow()
	},
	(t) => [
		uniqueIndex('org_roles_builtin_uidx')
			.on(t.orgId, t.builtin)
			.where(sql`${t.builtin} is not null`),
		uniqueIndex('org_roles_name_uidx').on(t.orgId, sql`lower(${t.name})`)
	]
);

export const orgMembers = pgTable(
	'org_members',
	{
		orgId: text('org_id')
			.notNull()
			.references(() => organizations.id, { onDelete: 'cascade' }),
		userId: text('user_id')
			.notNull()
			.references(() => user.id, { onDelete: 'cascade' }),
		/** owner: manages the org, its servers, members and invites, admin everywhere in it. member: per-server grants. */
		role: text('role', { enum: ['owner', 'member'] }).notNull(),
		/** the invite link they joined through, if any */
		inviteId: text('invite_id'),
		createdAt: ts('created_at').notNull().defaultNow()
	},
	(t) => [primaryKey({ columns: [t.orgId, t.userId] }), index('org_members_user_idx').on(t.userId)]
);

/** Shareable join links: <ORIGIN>/join/<token>. */
export const orgInvites = pgTable(
	'org_invites',
	{
		id: text('id').primaryKey(),
		orgId: text('org_id')
			.notNull()
			.references(() => organizations.id, { onDelete: 'cascade' }),
		token: text('token').notNull().unique(),
		label: text('label').notNull().default(''),
		orgRole: text('org_role', { enum: ['owner', 'member'] })
			.notNull()
			.default('member'),
		/** granted on every server the org has at join time; null = no server access until an owner grants it */
		serverRoleId: text('server_role_id').references(() => orgRoles.id, { onDelete: 'set null' }),
		/** null = unlimited */
		maxUses: integer('max_uses'),
		uses: integer('uses').notNull().default(0),
		/** null = never */
		expiresAt: ts('expires_at'),
		revokedAt: ts('revoked_at'),
		createdBy: text('created_by'),
		createdAt: ts('created_at').notNull().defaultNow()
	},
	(t) => [index('org_invites_org_idx').on(t.orgId)]
);

/**
 * Bearer credentials for bots and scripts, owned by an organisation. A key carries its own
 * capability set and an optional server allowlist (null = every org server, present and future);
 * it can never manage the org. Only the SHA-256 of the token is stored; the token is shown once.
 */
export const apiKeys = pgTable(
	'api_keys',
	{
		id: text('id').primaryKey(),
		orgId: text('org_id')
			.notNull()
			.references(() => organizations.id, { onDelete: 'cascade' }),
		label: text('label').notNull(),
		keyHash: text('key_hash').notNull().unique(),
		/** what the UI shows instead of the token: the prefix and the first few characters */
		hint: text('hint').notNull(),
		/** Capability[] */
		capabilities: jsonb('capabilities').notNull(),
		/** null = every server in the org */
		serverIds: jsonb('server_ids'),
		createdBy: text('created_by').references(() => user.id, { onDelete: 'set null' }),
		createdAt: ts('created_at').notNull().defaultNow(),
		lastUsedAt: ts('last_used_at'),
		expiresAt: ts('expires_at'),
		revokedAt: ts('revoked_at')
	},
	(t) => [index('api_keys_org_idx').on(t.orgId)]
);

export const servers = pgTable('servers', {
	id: text('id').primaryKey(),
	orgId: text('org_id')
		.notNull()
		.references(() => organizations.id, { onDelete: 'cascade' }),
	name: text('name').notNull(),
	host: text('host').notNull(),
	port: integer('port').notNull(),
	scheme: text('scheme', { enum: ['http', 'https'] })
		.notNull()
		.default('http'),
	/** AES-GCM, see crypto.ts */
	passwordEnc: text('password_enc').notNull(),
	notes: text('notes').notNull().default(''),
	sortOrder: integer('sort_order').notNull().default(0),
	/** Set when the site owner saved the target: private addresses (same box, LAN) are permitted. */
	allowPrivate: boolean('allow_private').notNull().default(false),
	/**
	 * The kill feed token the game sends as its bearer (`[WDServerFeed] Token`), AES-GCM like the
	 * password so it can be shown again and written into the config document; null = no feed.
	 */
	feedTokenEnc: text('feed_token_enc'),
	/** sha256 of the token: how a feed batch finds its server */
	feedTokenHash: text('feed_token_hash').unique(),
	/** the org owner's switches for the public pages; effective only with the org's allowance ($lib/features) */
	publicStatus: boolean('public_status').notNull().default(false),
	publicLeaderboards: boolean('public_leaderboards').notNull().default(false),
	/** the public status page also shows the last kills (needs the feed and the status page on) */
	publicKills: boolean('public_kills').notNull().default(false),
	createdBy: text('created_by'),
	createdAt: ts('created_at').notNull().defaultNow(),
	updatedAt: ts('updated_at').notNull().defaultNow()
});

export const serverGrants = pgTable(
	'server_grants',
	{
		serverId: text('server_id')
			.notNull()
			.references(() => servers.id, { onDelete: 'cascade' }),
		userId: text('user_id')
			.notNull()
			.references(() => user.id, { onDelete: 'cascade' }),
		/** must belong to the server's org; the writers in orgs.ts / users.ts / servers.ts check this */
		roleId: text('role_id')
			.notNull()
			.references(() => orgRoles.id, { onDelete: 'restrict' }),
		grantedBy: text('granted_by'),
		createdAt: ts('created_at').notNull().defaultNow()
	},
	(t) => [
		primaryKey({ columns: [t.serverId, t.userId] }),
		index('server_grants_user_idx').on(t.userId),
		index('server_grants_role_idx').on(t.roleId)
	]
);

export const auditLog = pgTable(
	'audit_log',
	{
		id: bigserial('id', { mode: 'number' }).primaryKey(),
		ts: ts('ts').notNull().defaultNow(),
		actorId: text('actor_id'),
		actorName: text('actor_name').notNull().default(''),
		serverId: text('server_id'),
		serverName: text('server_name').notNull().default(''),
		/** the organisation an event belongs to; org owners see these rows, not just their own */
		orgId: text('org_id'),
		/** auth | user | org | server | rcon | system */
		category: text('category').notNull(),
		/** e.g. login, rcon.kick, config.apply */
		action: text('action').notNull(),
		target: text('target').notNull().default(''),
		/** secrets redacted before insert */
		detail: jsonb('detail'),
		outcome: text('outcome', { enum: ['ok', 'error', 'denied'] }).notNull(),
		status: integer('status'),
		message: text('message').notNull().default(''),
		userAgent: text('user_agent').notNull().default(''),
		durationMs: integer('duration_ms')
	},
	(t) => [
		index('audit_ts_idx').on(t.ts),
		index('audit_server_idx').on(t.serverId, t.id),
		index('audit_org_idx').on(t.orgId, t.id),
		index('audit_actor_idx').on(t.actorId, t.id),
		index('audit_action_idx').on(t.category, t.action)
	]
);

export const loginAttempts = pgTable('login_attempts', {
	/** 'u:<username>', or 'ip:' / 'signup:' + a keyed hash of the address (addressKey in http.ts) */
	key: text('key').primaryKey(),
	count: integer('count').notNull().default(0),
	firstAt: ts('first_at').notNull(),
	lockedUntil: ts('locked_until')
});

// ---- Analytics (written by the poller; samples becomes a TimescaleDB hypertable) ---------------

export const samples = pgTable(
	'samples',
	{
		ts: ts('ts')
			.notNull()
			.default(sql`now()`),
		serverId: text('server_id').notNull(),
		ok: boolean('ok').notNull(),
		playerCount: integer('player_count'),
		maxPlayers: integer('max_players'),
		map: text('map'),
		/** "a+b" */
		experiences: text('experiences'),
		lighting: text('lighting'),
		matchSeconds: integer('match_seconds'),
		/** [{ name, score }] */
		scores: jsonb('scores'),
		/** [{ name, cash }]: cash held per faction ('' = unassigned), summed over connected players */
		cash: jsonb('cash'),
		latencyMs: integer('latency_ms'),
		error: text('error')
	},
	(t) => [index('samples_server_ts_idx').on(t.serverId, t.ts.desc())]
);

export const playerSessions = pgTable(
	'player_sessions',
	{
		id: bigserial('id', { mode: 'number' }).primaryKey(),
		serverId: text('server_id').notNull(),
		steamId: text('steam_id').notNull(),
		name: text('name').notNull(),
		faction: text('faction'),
		joinedAt: ts('joined_at').notNull(),
		lastSeen: ts('last_seen').notNull(),
		/** null while online */
		leftAt: ts('left_at'),
		kills: integer('kills').notNull().default(0),
		deaths: integer('deaths').notNull().default(0),
		/** cash, banked across the session's matches like kills: the scoreboard starts it again
		 *  with the counters (followPlayer) */
		cash: integer('cash').notNull().default(0),
		/** seconds of this session spent with the player count at or under the server's seeding
		 *  threshold (0 while no seeding rule is on); what a Seeding reward rule adds up */
		seedSeconds: integer('seed_seconds').notNull().default(0)
	},
	(t) => [
		index('player_sessions_open_idx').on(t.serverId, t.leftAt),
		index('player_sessions_seen_idx').on(t.serverId, t.lastSeen),
		index('player_sessions_steam_idx').on(t.steamId, t.joinedAt)
	]
);

/** Materialized identity for an organisation. Session and kill rows remain the source of stats. */
export const integrityProfiles = pgTable(
	'integrity_profiles',
	{
		orgId: text('org_id')
			.notNull()
			.references(() => organizations.id, { onDelete: 'cascade' }),
		steamId: text('steam_id').notNull(),
		currentName: text('current_name').notNull(),
		aliases: jsonb('aliases').notNull(),
		firstSeen: ts('first_seen').notNull(),
		lastSeen: ts('last_seen').notNull()
	},
	(t) => [
		primaryKey({ columns: [t.orgId, t.steamId] }),
		index('integrity_profiles_last_seen_idx').on(t.orgId, t.lastSeen.desc())
	]
);

/** Organisation overrides for exact cause tags; unknown causes are never assumed infantry. */
export const integrityWeaponMap = pgTable(
	'integrity_weapon_map',
	{
		orgId: text('org_id')
			.notNull()
			.references(() => organizations.id, { onDelete: 'cascade' }),
		cause: text('cause').notNull(),
		category: text('category').notNull(),
		updatedBy: text('updated_by'),
		updatedAt: ts('updated_at').notNull().defaultNow()
	},
	(t) => [primaryKey({ columns: [t.orgId, t.cause] })]
);

/** Statistical generations are independent of owner-editable legacy score rules. */
export const integrityModelState = pgTable('integrity_model_state', {
	orgId: text('org_id')
		.primaryKey()
		.references(() => organizations.id, { onDelete: 'cascade' }),
	weaponMapVersion: integer('weapon_map_version').notNull().default(1),
	activeBaselineGeneration: text('active_baseline_generation'),
	baselineStatus: text('baseline_status').notNull().default('STALE'),
	lastRefreshAt: ts('last_refresh_at'),
	lastFailureAt: ts('last_failure_at'),
	lastDurationMs: integer('last_duration_ms'),
	updatedAt: ts('updated_at').notNull().defaultNow()
});

/** Non-overlapping abnormal infantry windows; no action is implied by a row. */
export const integrityWindows = pgTable(
	'integrity_windows',
	{
		id: bigserial('id', { mode: 'number' }).primaryKey(),
		orgId: text('org_id').notNull(),
		serverId: text('server_id').notNull(),
		steamId: text('steam_id').notNull(),
		instanceId: text('instance_id').notNull(),
		roundId: text('round_id'),
		map: text('map').notNull(),
		clockFrom: real('clock_from').notNull(),
		clockTo: real('clock_to').notNull(),
		observedAt: ts('observed_at').notNull(),
		infantryKills: integer('infantry_kills').notNull(),
		kpm180: real('kpm_180').notNull(),
		uniqueVictims: integer('unique_victims').notNull(),
		headshots: integer('headshots').notNull().default(0),
		penetrations: integer('penetrations').notNull().default(0),
		burstPoints: integer('burst_points').notNull().default(0),
		maxKills15s: integer('max_kills_15s'),
		medianKillInterval: real('median_kill_interval'),
		behaviorReasons: jsonb('behavior_reasons').notNull().default([]),
		eventIds: jsonb('event_ids').notNull()
	},
	(t) => [index('integrity_windows_player_idx').on(t.orgId, t.steamId, t.observedAt.desc())]
);

/** Empirical 30-day summaries; exact value frequencies remain server-side for ranking. */
export const integrityBaselines = pgTable(
	'integrity_baselines',
	{
		id: text('id').primaryKey(),
		orgId: text('org_id')
			.notNull()
			.references(() => organizations.id, { onDelete: 'cascade' }),
		serverId: text('server_id'),
		metric: text('metric').notNull(),
		level: integer('level').notNull(),
		map: text('map'),
		populationBucket: text('population_bucket'),
		weaponCategory: text('weapon_category').notNull(),
		/** Local feed or approved, user-supplied external history. Never mix their distributions. */
		source: text('source').notNull().default('local'),
		sampleCount: integer('sample_count').notNull(),
		uniquePlayers: integer('unique_players').notNull().default(0),
		uniquePlayerDays: integer('unique_player_days').notNull().default(0),
		effectiveSampleSize: real('effective_sample_size').notNull().default(0),
		modelVersion: text('model_version').notNull().default('legacy-fixed-v0'),
		featureVersion: text('feature_version').notNull().default('fixed-slot-v0'),
		weaponMapVersion: integer('weapon_map_version').notNull().default(0),
		generation: text('generation').notNull().default('legacy'),
		median: real('median').notNull(),
		mad: real('mad'),
		p90: real('p90').notNull(),
		p95: real('p95').notNull(),
		p99: real('p99').notNull(),
		p995: real('p995').notNull(),
		p999: real('p999').notNull(),
		p9995: real('p9995').notNull(),
		histogram: jsonb('histogram').notNull(),
		cdf: jsonb('cdf').notNull(),
		windowDays: integer('window_days').notNull().default(30),
		calculatedAt: ts('calculated_at').notNull()
	},
	(t) => [
		index('integrity_baselines_lookup_idx').on(t.orgId, t.metric, t.level),
		index('integrity_baselines_server_idx').on(t.orgId, t.serverId, t.metric)
	]
);

/** Only safe/normal windows enter this append-only career reference history. */
export const integrityPlayerMetricHistory = pgTable(
	'integrity_player_metric_history',
	{
		id: bigserial('id', { mode: 'number' }).primaryKey(),
		orgId: text('org_id')
			.notNull()
			.references(() => organizations.id, { onDelete: 'cascade' }),
		steamId: text('steam_id').notNull(),
		serverId: text('server_id').notNull(),
		roundId: text('round_id').notNull(),
		eventId: text('event_id').notNull(),
		observedAt: ts('observed_at').notNull(),
		kpm180: real('kpm_180').notNull(),
		headshotRate: real('headshot_rate'),
		maxKills15s: integer('max_kills_15s').notNull(),
		featureVersion: text('feature_version').notNull(),
		modelVersion: text('model_version').notNull()
	},
	(t) => [
		uniqueIndex('integrity_player_history_event_idx').on(t.orgId, t.serverId, t.eventId),
		index('integrity_player_history_player_idx').on(t.orgId, t.steamId, t.observedAt.desc())
	]
);

/** Bounded materialized personal reference; raw career totals are descriptive, never votes. */
export const integrityPlayerCareers = pgTable(
	'integrity_player_careers',
	{
		orgId: text('org_id')
			.notNull()
			.references(() => organizations.id, { onDelete: 'cascade' }),
		steamId: text('steam_id').notNull(),
		firstSeenAt: ts('first_seen_at').notNull(),
		lastSeenAt: ts('last_seen_at').notNull(),
		lifetimeValidKills: integer('lifetime_valid_kills'),
		lifetimePlaytimeSeconds: integer('lifetime_playtime_seconds'),
		lifetimeWindows: integer('lifetime_windows').notNull(),
		lifetimeMatches: integer('lifetime_matches').notNull(),
		activeDays: integer('active_days').notNull(),
		kpmDistribution: jsonb('kpm_distribution').notNull(),
		headshotDistribution: jsonb('headshot_distribution'),
		burstDistribution: jsonb('burst_distribution').notNull(),
		recent24h: jsonb('recent_24h'),
		recent7d: jsonb('recent_7d'),
		recent30d: jsonb('recent_30d'),
		orderedKpm: jsonb('ordered_kpm').notNull(),
		modelVersion: text('model_version').notNull(),
		featureVersion: text('feature_version').notNull(),
		status: text('status').notNull().default('READY'),
		updatedAt: ts('updated_at').notNull().defaultNow()
	},
	(t) => [primaryKey({ columns: [t.orgId, t.steamId] })]
);

/** Human corrections are labels for offline calibration, never live score adjustments. */
export const integrityLabels = pgTable(
	'integrity_labels',
	{
		id: bigserial('id', { mode: 'number' }).primaryKey(),
		caseId: text('case_id').notNull(),
		orgId: text('org_id')
			.notNull()
			.references(() => organizations.id, { onDelete: 'cascade' }),
		label: text('label').notNull(),
		reason: text('reason').notNull(),
		reviewerId: text('reviewer_id').notNull(),
		modelVersion: text('model_version').notNull(),
		createdAt: ts('created_at').notNull().defaultNow()
	},
	(t) => [index('integrity_labels_case_idx').on(t.caseId, t.createdAt.desc())]
);

/** Independent, bounded Steam enrichment; never awaited by the ordered feed consumer. */
export const integrityProfileRefreshJobs = pgTable(
	'integrity_profile_refresh_jobs',
	{
		steamId: text('steam_id').primaryKey(),
		state: text('state').notNull().default('pending'),
		attempts: integer('attempts').notNull().default(0),
		nextAt: ts('next_at').notNull().defaultNow(),
		leaseUntil: ts('lease_until'),
		lastError: text('last_error'),
		updatedAt: ts('updated_at').notNull().defaultNow()
	},
	(t) => [index('integrity_profile_refresh_pending_idx').on(t.state, t.nextAt)]
);

/** External history is reviewed before it can influence a baseline; it never enters the live kill feed. */
export const integrityImportBatches = pgTable(
	'integrity_import_batches',
	{
		id: text('id').primaryKey(),
		orgId: text('org_id')
			.notNull()
			.references(() => organizations.id, { onDelete: 'cascade' }),
		sourceServer: text('source_server').notNull(),
		fileSha256: text('file_sha256').notNull(),
		status: text('status').notNull().default('STAGED'),
		rowCount: integer('row_count').notNull(),
		firstEventAt: ts('first_event_at').notNull(),
		lastEventAt: ts('last_event_at').notNull(),
		stagedAt: ts('staged_at').notNull().defaultNow(),
		stagedBy: text('staged_by').notNull(),
		reviewedAt: ts('reviewed_at'),
		reviewedBy: text('reviewed_by')
	},
	(t) => [
		index('integrity_import_batches_org_idx').on(t.orgId, t.stagedAt.desc()),
		uniqueIndex('integrity_import_batches_hash_idx').on(t.orgId, t.fileSha256)
	]
);

export const integrityImportKills = pgTable(
	'integrity_import_kills',
	{
		batchId: text('batch_id')
			.notNull()
			.references(() => integrityImportBatches.id, { onDelete: 'cascade' }),
		orgId: text('org_id').notNull(),
		sourceServer: text('source_server').notNull(),
		eventId: text('event_id').notNull(),
		eventAt: ts('event_at').notNull(),
		instanceId: text('instance_id').notNull(),
		matchId: text('match_id').notNull(),
		eventTime: real('event_time').notNull(),
		map: text('map').notNull(),
		killerSteamId: text('killer_steam_id').notNull(),
		victimSteamId: text('victim_steam_id').notNull(),
		killerFaction: text('killer_faction').notNull(),
		victimFaction: text('victim_faction').notNull(),
		cause: text('cause').notNull(),
		distanceM: real('distance_m'),
		headshot: boolean('headshot').notNull(),
		penetration: boolean('penetration').notNull(),
		playerCount: integer('player_count')
	},
	(t) => [
		uniqueIndex('integrity_import_kills_source_event_idx').on(t.orgId, t.sourceServer, t.eventId),
		index('integrity_import_kills_batch_idx').on(t.batchId),
		index('integrity_import_kills_event_at_idx').on(t.orgId, t.eventAt)
	]
);

/** A versioned, owner-editable rule set. Cases snapshot both version and effective inputs. */
export const integrityRules = pgTable('integrity_rules', {
	orgId: text('org_id')
		.primaryKey()
		.references(() => organizations.id, { onDelete: 'cascade' }),
	version: integer('version').notNull().default(1),
	config: jsonb('config').notNull(),
	assessmentMode: text('assessment_mode').notNull().default('statistical_shadow'),
	autoKickEnabled: boolean('auto_kick_enabled').notNull().default(false),
	autoQuarantine24hEnabled: boolean('auto_quarantine_24h_enabled').notNull().default(false),
	autoQuarantine7dEnabled: boolean('auto_quarantine_7d_enabled').notNull().default(false),
	autoActionMaxPerHour: integer('auto_action_max_per_hour').notNull().default(10),
	autoActionMaxPercentOnline: integer('auto_action_max_percent_online').notNull().default(10),
	autoSuspendedAt: ts('auto_suspended_at'),
	updatedBy: text('updated_by'),
	updatedAt: ts('updated_at').notNull().defaultNow()
});

/** Explainable, non-enforcing score created from a persisted abnormal window. */
export const integrityScores = pgTable(
	'integrity_scores',
	{
		id: bigserial('id', { mode: 'number' }).primaryKey(),
		windowId: bigint('window_id', { mode: 'number' }),
		reportId: bigint('report_id', { mode: 'number' }),
		source: text('source').notNull().default('window'),
		orgId: text('org_id').notNull(),
		serverId: text('server_id').notNull(),
		steamId: text('steam_id').notNull(),
		scoredAt: ts('scored_at').notNull(),
		ruleVersion: integer('rule_version').notNull(),
		score: integer('score').notNull(),
		level: text('level').notNull(),
		breakdown: jsonb('breakdown').notNull(),
		statistical: jsonb('statistical'),
		currentBehaviorAnomaly: boolean('current_behavior_anomaly').notNull()
	},
	(t) => [
		index('integrity_scores_player_idx').on(t.orgId, t.steamId, t.scoredAt.desc()),
		check(
			'integrity_scores_source_check',
			sql`(${t.source} = 'window' AND ${t.windowId} IS NOT NULL AND ${t.reportId} IS NULL) OR (${t.source} = 'report' AND ${t.reportId} IS NOT NULL AND ${t.windowId} IS NULL)`
		)
	]
);

/** Immutable evidence snapshots; review state lives alongside, raw feed rows stay in kills. */
export const integrityCases = pgTable(
	'integrity_cases',
	{
		id: text('id').primaryKey(),
		orgId: text('org_id').notNull(),
		serverId: text('server_id').notNull(),
		steamId: text('steam_id').notNull(),
		createdAt: ts('created_at').notNull(),
		status: text('status').notNull().default('OPEN'),
		confidence: text('confidence').notNull(),
		trigger: text('trigger').notNull(),
		ruleVersion: integer('rule_version').notNull(),
		riskScore: integer('risk_score').notNull(),
		riskBreakdown: jsonb('risk_breakdown').notNull(),
		scoreId: bigint('score_id', { mode: 'number' }).references(() => integrityScores.id),
		statistical: jsonb('statistical'),
		snapshot: jsonb('snapshot').notNull(),
		reviewedBy: text('reviewed_by'),
		reviewedAt: ts('reviewed_at')
	},
	(t) => [
		index('integrity_cases_queue_idx').on(t.orgId, t.status, t.createdAt.desc()),
		index('integrity_cases_player_idx').on(t.orgId, t.steamId, t.createdAt.desc())
	]
);

/** Assessment transition and retryable action eligibility have distinct durable state. */
export const integrityActionEligibility = pgTable('integrity_action_eligibility', {
	caseId: text('case_id')
		.primaryKey()
		.references(() => integrityCases.id, { onDelete: 'cascade' }),
	lastAttemptAt: ts('last_attempt_at').notNull(),
	attempts: integer('attempts').notNull().default(1)
});

/** Source event rows for new cases; old JSON snapshots remain readable. */
export const integrityCaseEvents = pgTable(
	'integrity_case_events',
	{
		caseId: text('case_id')
			.notNull()
			.references(() => integrityCases.id, { onDelete: 'cascade' }),
		instanceId: text('instance_id').notNull(),
		eventId: text('event_id').notNull(),
		event: jsonb('event').notNull()
	},
	(t) => [primaryKey({ columns: [t.caseId, t.instanceId, t.eventId] })]
);

/** Explicit origin and expiry for experimental rule actions. */
export const integrityActions = pgTable(
	'integrity_actions',
	{
		id: text('id').primaryKey(),
		caseId: text('case_id')
			.notNull()
			.references(() => integrityCases.id, { onDelete: 'cascade' }),
		orgId: text('org_id').notNull(),
		serverId: text('server_id').notNull(),
		steamId: text('steam_id').notNull(),
		action: text('action').notNull(),
		source: text('source').notNull(),
		listEntryId: text('list_entry_id'),
		createdAt: ts('created_at').notNull().defaultNow(),
		/** NULL until a kick is confirmed; quarantine becomes effective with the panel list entry. */
		effectiveAt: ts('effective_at'),
		/** State of the immediate RCON kick, separate from quarantine list effectiveness. */
		deliveryState: text('delivery_state'),
		expiresAt: ts('expires_at'),
		revertedAt: ts('reverted_at'),
		revertedBy: text('reverted_by')
	},
	(t) => [index('integrity_actions_player_idx').on(t.orgId, t.steamId, t.createdAt.desc())]
);

/** A unique Steam reporter may file again after cooldown, but cannot inflate risk by repetition. */
export const integrityReports = pgTable(
	'integrity_reports',
	{
		id: bigserial('id', { mode: 'number' }).primaryKey(),
		orgId: text('org_id').notNull(),
		serverId: text('server_id').notNull(),
		targetSteamId: text('target_steam_id').notNull(),
		reporterSteamId: text('reporter_steam_id').notNull(),
		reason: text('reason').notNull(),
		source: text('source').notNull(),
		createdAt: ts('created_at').notNull(),
		evidenceFrom: ts('evidence_from').notNull(),
		evidenceUntil: ts('evidence_until').notNull(),
		caseId: text('case_id'),
		status: text('status').notNull().default('OPEN')
	},
	(t) => [
		index('integrity_reports_target_idx').on(t.orgId, t.targetSteamId, t.createdAt.desc()),
		index('integrity_reports_reporter_idx').on(t.reporterSteamId, t.createdAt.desc())
	]
);

/** Frozen copies of accepted telemetry surrounding a report, including late events. */
export const integrityReportEvents = pgTable(
	'integrity_report_events',
	{
		reportId: bigint('report_id', { mode: 'number' }).notNull(),
		instanceId: text('instance_id').notNull(),
		eventId: text('event_id').notNull(),
		receivedAt: ts('received_at').notNull(),
		event: jsonb('event').notNull()
	},
	(t) => [primaryKey({ columns: [t.reportId, t.instanceId, t.eventId] })]
);

export const integrityReporterStats = pgTable(
	'integrity_reporter_stats',
	{
		orgId: text('org_id').notNull(),
		steamId: text('steam_id').notNull(),
		reportsSubmitted: integer('reports_submitted').notNull().default(0),
		reportsConfirmed: integer('reports_confirmed').notNull().default(0),
		reportsDismissed: integer('reports_dismissed').notNull().default(0)
	},
	(t) => [primaryKey({ columns: [t.orgId, t.steamId] })]
);

export const matches = pgTable(
	'matches',
	{
		id: bigserial('id', { mode: 'number' }).primaryKey(),
		serverId: text('server_id').notNull(),
		/** estimated from matchSeconds at first sight */
		startedAt: ts('started_at').notNull(),
		/** null while in progress */
		endedAt: ts('ended_at'),
		map: text('map'),
		experiences: text('experiences'),
		lighting: text('lighting'),
		peakPlayers: integer('peak_players').notNull().default(0),
		/** [{ name, score }] */
		finalScores: jsonb('final_scores'),
		awardSnapshot: jsonb('award_snapshot'),
		winner: text('winner')
	},
	(t) => [index('matches_server_idx').on(t.serverId, t.startedAt)]
);

/**
 * One row per kill the game's feed delivered (`[WDServerFeed]`, see docs/wardogs-api.md), with
 * what Warcon knew at receipt: the open match and both players' factions. History: never pruned;
 * a TimescaleDB hypertable with compression where the extension exists (migration 0019).
 */
export const kills = pgTable(
	'kills',
	{
		/** when Warcon received it, a second or two after the kill */
		ts: ts('ts').notNull(),
		serverId: text('server_id').notNull(),
		eventId: text('event_id').notNull(),
		/** the game's serverId: a per-boot instance id, not the join code */
		instanceId: text('instance_id').notNull(),
		/** the game's matchId: also per boot, as observed */
		matchId: text('match_id').notNull(),
		/** matches.id open on this server at receipt */
		matchRow: bigint('match_row', { mode: 'number' }),
		/** seconds on the match clock */
		eventTime: real('event_time').notNull(),
		map: text('map').notNull(),
		/** null: the environment */
		killerSteamId: text('killer_steam_id'),
		killerName: text('killer_name'),
		killerFaction: text('killer_faction'),
		/** Time of the fresh player-list look used for both factions; null on older/session-derived rows. */
		factionObservedAt: ts('faction_observed_at'),
		/** Both sides were unchanged across feed receipt and the next player observation. */
		factionBracketed: boolean('faction_bracketed').notNull().default(false),
		victimSteamId: text('victim_steam_id').notNull(),
		victimName: text('victim_name').notNull(),
		victimFaction: text('victim_faction'),
		/** the raw weapon or vehicle tag, e.g. Id.Item.AK74M */
		cause: text('cause'),
		distanceM: real('distance_m'),
		distanceInvalid: boolean('distance_invalid').notNull().default(false),
		rawDistanceCm: real('raw_distance_cm'),
		headshot: boolean('headshot').notNull().default(false),
		/** the Suicide tag, or killer = victim */
		suicide: boolean('suicide').notNull().default(false),
		/** both factions known and equal, killer ≠ victim */
		teamKill: boolean('team_kill').notNull().default(false),
		/** the other context tags, short form: Penetration, Ricochet, RoadKill, VehicleExplosion, Falling, WeaponMelee */
		tags: jsonb('tags').notNull()
	},
	(t) => [
		// Not unique: a hypertable's unique indexes must include ts, so dedupe is a lookup (feed.ts).
		index('kills_event_idx').on(t.eventId),
		index('kills_server_ts_idx').on(t.serverId, t.ts.desc()),
		index('kills_killer_idx').on(t.killerSteamId, t.ts.desc()),
		index('kills_victim_idx').on(t.victimSteamId, t.ts.desc())
	]
);
export type KillRow = typeof kills.$inferSelect;

/** Durable handoff from feed ingestion to the worker; unrelated to RCON outbox. */
export const feedProcessingJobs = pgTable(
	'feed_processing_jobs',
	{
		id: bigserial('id', { mode: 'number' }).primaryKey(),
		serverId: text('server_id')
			.notNull()
			.references(() => servers.id, { onDelete: 'cascade' }),
		killTs: ts('kill_ts').notNull(),
		eventIds: jsonb('event_ids').notNull(),
		createdAt: ts('created_at').notNull().defaultNow(),
		consumer: text('consumer').notNull().default('legacy'),
		state: text('state').notNull().default('pending'),
		attempts: integer('attempts').notNull().default(0),
		leaseUntil: ts('lease_until'),
		doneAt: ts('done_at'),
		lastError: text('last_error')
	},
	(t) => [index('feed_processing_pending_idx').on(t.consumer, t.state, t.createdAt)]
);
export type FeedProcessingJob = typeof feedProcessingJobs.$inferSelect;

/**
 * One row per player per match: the game's own scoreboard counters over the match (kills, deaths,
 * the change in cash) with the player's time on and side, and, on servers with a kill feed, what
 * the feed adds (headshots, team kills, suicides, vehicle kills, longest shot, best streaks).
 * Written by the worker when a player leaves and at the match end (match-players.ts); boards,
 * careers, the dossier and analytics read these rather than the sessions or the feed. Kept for
 * good; a server's stats purge deletes them with its matches. No foreign key, like kills.
 */
export const matchPlayers = pgTable(
	'match_players',
	{
		matchId: bigint('match_id', { mode: 'number' }).notNull(),
		serverId: text('server_id').notNull(),
		steamId: text('steam_id').notNull(),
		/** the name at the last look */
		name: text('name').notNull(),
		/** the last side seen that was a team on the scoreboard */
		faction: text('faction'),
		/** time on during the match */
		seconds: integer('seconds').notNull().default(0),
		kills: integer('kills').notNull().default(0),
		deaths: integer('deaths').notNull().default(0),
		/** the cash earned over the match: each run of the counters' last look less its first */
		cashDelta: integer('cash_delta').notNull().default(0),
		headshots: integer('headshots').notNull().default(0),
		teamKills: integer('team_kills').notNull().default(0),
		suicides: integer('suicides').notNull().default(0),
		vehicleKills: integer('vehicle_kills').notNull().default(0),
		/** null without a feed or a distance */
		longestM: real('longest_m'),
		killStreak: integer('kill_streak').notNull().default(0),
		deathStreak: integer('death_streak').notNull().default(0)
	},
	(t) => [
		primaryKey({ columns: [t.matchId, t.steamId] }),
		index('match_players_server_idx').on(t.serverId, t.matchId),
		index('match_players_steam_idx').on(t.steamId, t.matchId)
	]
);
export type MatchPlayerRow = typeof matchPlayers.$inferSelect;

// ---- Player intelligence: org-scoped notes and watchlist, cached Steam data, ban snapshots ------

/** One row per (org, player): the watchlist flag and why. */
export const playerMarks = pgTable(
	'player_marks',
	{
		orgId: text('org_id')
			.notNull()
			.references(() => organizations.id, { onDelete: 'cascade' }),
		steamId: text('steam_id').notNull(),
		watched: boolean('watched').notNull().default(false),
		reason: text('reason').notNull().default(''),
		updatedBy: text('updated_by'),
		updatedByName: text('updated_by_name').notNull().default(''),
		updatedAt: ts('updated_at').notNull().defaultNow()
	},
	(t) => [primaryKey({ columns: [t.orgId, t.steamId] })]
);

/** Free-text notes admins leave on a player, shared across the org's servers. */
export const playerNotes = pgTable(
	'player_notes',
	{
		id: bigserial('id', { mode: 'number' }).primaryKey(),
		orgId: text('org_id')
			.notNull()
			.references(() => organizations.id, { onDelete: 'cascade' }),
		steamId: text('steam_id').notNull(),
		authorId: text('author_id'),
		authorName: text('author_name').notNull().default(''),
		body: text('body').notNull(),
		createdAt: ts('created_at').notNull().defaultNow()
	},
	(t) => [index('player_notes_idx').on(t.orgId, t.steamId, t.id)]
);

/** What the Steam Web API last said about a SteamID (persona, account age, VAC and game bans). */
export const steamProfiles = pgTable('steam_profiles', {
	steamId: text('steam_id').primaryKey(),
	persona: text('persona').notNull().default(''),
	avatar: text('avatar').notNull().default(''),
	profileUrl: text('profile_url').notNull().default(''),
	/** community visibility: only public profiles expose the creation date */
	public: boolean('public').notNull().default(false),
	accountCreatedAt: ts('account_created_at'),
	vacBans: integer('vac_bans').notNull().default(0),
	gameBans: integer('game_bans').notNull().default(0),
	daysSinceLastBan: integer('days_since_last_ban'),
	communityBanned: boolean('community_banned').notNull().default(false),
	economyBan: text('economy_ban').notNull().default('none'),
	/** unknown, public, private, or partial (only the first 200 friends checked) */
	friendsState: text('friends_state').notNull().default('unknown'),
	friendsTotal: integer('friends_total').notNull().default(0),
	friendsChecked: integer('friends_checked').notNull().default(0),
	bannedFriends: integer('banned_friends').notNull().default(0),
	friendsCheckedAt: ts('friends_checked_at'),
	fetchedAt: ts('fetched_at').notNull().defaultNow(),
	error: text('error').notNull().default('')
});

/** The poller's copy of each game server's ban list, so bans on one server are visible from another. */
export const serverBans = pgTable(
	'server_bans',
	{
		serverId: text('server_id')
			.notNull()
			.references(() => servers.id, { onDelete: 'cascade' }),
		steamId: text('steam_id').notNull(),
		reason: text('reason').notNull().default(''),
		bannedBy: text('banned_by').notNull().default(''),
		bannedAtUtc: text('banned_at_utc').notNull().default(''),
		seenAt: ts('seen_at').notNull().defaultNow()
	},
	(t) => [
		primaryKey({ columns: [t.serverId, t.steamId] }),
		index('server_bans_steam_idx').on(t.steamId)
	]
);

// ---- Automation: per-server triggers run by the poller ----------------------------------------

export const triggers = pgTable(
	'triggers',
	{
		id: text('id').primaryKey(),
		serverId: text('server_id')
			.notNull()
			.references(() => servers.id, { onDelete: 'cascade' }),
		orgId: text('org_id').notNull(),
		kind: text('kind', {
			enum: [
				'welcome',
				'faction_change',
				'broadcast',
				'empty_reset',
				'risk_kick',
				'ping_kick',
				'restart_notice',
				'team_kill',
				'seed_reward',
				'match_broadcast',
				'name_filter',
				'kill_rate'
			]
		}).notNull(),
		name: text('name').notNull(),
		enabled: boolean('enabled').notNull().default(false),
		/** kind-specific settings, validated in triggers.ts */
		config: jsonb('config').notNull(),
		/** kind-specific runtime state (e.g. the next broadcast index) */
		state: jsonb('state'),
		lastFiredAt: ts('last_fired_at'),
		lastResult: text('last_result').notNull().default(''),
		fireCount: integer('fire_count').notNull().default(0),
		createdBy: text('created_by'),
		createdAt: ts('created_at').notNull().defaultNow(),
		updatedAt: ts('updated_at').notNull().defaultNow()
	},
	(t) => [index('triggers_server_idx').on(t.serverId)]
);

// ---- Outbound: Discord webhooks that mirror the audit trail -----------------------------------

export const webhooks = pgTable(
	'webhooks',
	{
		id: text('id').primaryKey(),
		orgId: text('org_id')
			.notNull()
			.references(() => organizations.id, { onDelete: 'cascade' }),
		label: text('label').notNull().default(''),
		/** the webhook URL is a bearer credential; AES-GCM like RCON passwords */
		urlEnc: text('url_enc').notNull(),
		/** what the UI shows instead of the URL: host and webhook id */
		urlHint: text('url_hint').notNull().default(''),
		/** event classes to mirror; see webhook-delivery.ts */
		events: jsonb('events').notNull(),
		/** null = every server in the org */
		serverIds: jsonb('server_ids'),
		enabled: boolean('enabled').notNull().default(true),
		/** keep a live status message per covered server, edited in place; see webhook-status.ts */
		statusEnabled: boolean('status_enabled').notNull().default(false),
		/** how the cards look; see $lib/status-styles */
		statusStyle: text('status_style', { enum: ['banner', 'compact', 'scoreboard'] })
			.notNull()
			.default('banner'),
		/** seconds between edits of one card (30-300); the per-server spacing applies on top */
		statusIntervalS: integer('status_interval_s').notNull().default(60),
		/** which links the card carries: the public status page, the public leaderboard, the panel */
		linkStatus: boolean('link_status').notNull().default(true),
		linkLeaderboard: boolean('link_leaderboard').notNull().default(true),
		linkPanel: boolean('link_panel').notNull().default(false),
		/** server id -> the Discord id of its message, once posted */
		statusMessages: jsonb('status_messages'),
		statusSentAt: ts('status_sent_at'),
		lastSentAt: ts('last_sent_at'),
		lastStatus: integer('last_status'),
		lastError: text('last_error').notNull().default(''),
		createdBy: text('created_by'),
		createdAt: ts('created_at').notNull().defaultNow(),
		updatedAt: ts('updated_at').notNull().defaultNow()
	},
	(t) => [index('webhooks_org_idx').on(t.orgId)]
);

// ---- Organisation lists: bans and reserved slots kept in the panel and pushed to every server --

/**
 * A ban list or reserved-slot list an org owns. Servers subscribe through server_lists: every org
 * list to every org server, and a server's own list (server_id set) to that server alone.
 */
export const lists = pgTable(
	'lists',
	{
		id: text('id').primaryKey(),
		orgId: text('org_id')
			.notNull()
			.references(() => organizations.id, { onDelete: 'cascade' }),
		/** set on a list that belongs to one server (its own reserved slots); null for the org's */
		serverId: text('server_id').references(() => servers.id, { onDelete: 'cascade' }),
		kind: text('kind', { enum: ['ban', 'reserve'] }).notNull(),
		name: text('name').notNull().default('Default'),
		/** reserved for sharing between orgs; unused for now */
		shareToken: text('share_token').unique(),
		createdBy: text('created_by'),
		createdAt: ts('created_at').notNull().defaultNow(),
		updatedAt: ts('updated_at').notNull().defaultNow()
	},
	(t) => [
		uniqueIndex('lists_org_kind_name_uidx')
			.on(t.orgId, t.kind, t.name)
			.where(sql`${t.serverId} is null`),
		uniqueIndex('lists_server_kind_uidx')
			.on(t.serverId, t.kind)
			.where(sql`${t.serverId} is not null`)
	]
);

/** One player on a list. Removal is soft so history and audit stay intact; re-adding inserts a new row. */
export const listEntries = pgTable(
	'list_entries',
	{
		id: text('id').primaryKey(),
		listId: text('list_id')
			.notNull()
			.references(() => lists.id, { onDelete: 'cascade' }),
		steamId: text('steam_id').notNull(),
		reason: text('reason').notNull().default(''),
		/** bans only: lifted automatically after this */
		expiresAt: ts('expires_at'),
		addedBy: text('added_by'),
		addedByName: text('added_by_name').notNull().default(''),
		addedAt: ts('added_at').notNull().defaultNow(),
		removedAt: ts('removed_at'),
		removedBy: text('removed_by'),
		removedByName: text('removed_by_name').notNull().default(''),
		removal: text('removal', { enum: ['manual', 'expired'] })
	},
	(t) => [
		uniqueIndex('list_entries_active_uidx')
			.on(t.listId, t.steamId)
			.where(sql`${t.removedAt} is null`),
		index('list_entries_list_idx').on(t.listId, t.removedAt),
		index('list_entries_steam_idx').on(t.steamId)
	]
);

/** Which lists apply to which server: every org list to every org server, a server's own to itself. */
export const serverLists = pgTable(
	'server_lists',
	{
		serverId: text('server_id')
			.notNull()
			.references(() => servers.id, { onDelete: 'cascade' }),
		listId: text('list_id')
			.notNull()
			.references(() => lists.id, { onDelete: 'cascade' })
	},
	(t) => [
		primaryKey({ columns: [t.serverId, t.listId] }),
		index('server_lists_list_idx').on(t.listId)
	]
);

/** The poller's copy of each game server's reserved slots; sibling of server_bans. */
export const serverReserved = pgTable(
	'server_reserved',
	{
		serverId: text('server_id')
			.notNull()
			.references(() => servers.id, { onDelete: 'cascade' }),
		steamId: text('steam_id').notNull(),
		seenAt: ts('seen_at').notNull().defaultNow()
	},
	(t) => [primaryKey({ columns: [t.serverId, t.steamId] })]
);

/**
 * What Warcon itself put on a server, and from which list. Entries on the server with no row here
 * are "local" (added outside the panel) and are never removed by the sync.
 */
export const serverListState = pgTable(
	'server_list_state',
	{
		serverId: text('server_id')
			.notNull()
			.references(() => servers.id, { onDelete: 'cascade' }),
		kind: text('kind', { enum: ['ban', 'reserve'] }).notNull(),
		steamId: text('steam_id').notNull(),
		sourceListId: text('source_list_id').references(() => lists.id, { onDelete: 'set null' }),
		state: text('state', { enum: ['applied', 'failed'] }).notNull(),
		error: text('error').notNull().default(''),
		attemptedAt: ts('attempted_at'),
		updatedAt: ts('updated_at').notNull().defaultNow()
	},
	(t) => [
		primaryKey({ columns: [t.serverId, t.kind, t.steamId] }),
		index('server_list_state_source_idx').on(t.sourceListId)
	]
);

/** Per-server sync bookkeeping: last run and last error. */
export const serverListSync = pgTable('server_list_sync', {
	serverId: text('server_id')
		.primaryKey()
		.references(() => servers.id, { onDelete: 'cascade' }),
	syncedAt: ts('synced_at'),
	lastError: text('last_error').notNull().default(''),
	updatedAt: ts('updated_at').notNull().defaultNow()
});

// ---- live observation, trigger outbox, settings, worker ownership --------------------------------

/**
 * What the worker last saw on each server: one row per server, overwritten on every observation
 * that changed something (and on a heartbeat), so a page load is one indexed read.
 */
export const serverLive = pgTable('server_live', {
	serverId: text('server_id')
		.primaryKey()
		.references(() => servers.id, { onDelete: 'cascade' }),
	ok: boolean('ok').notNull().default(false),
	error: text('error').notNull().default(''),
	/** watched | hot | idle | offline */
	tier: text('tier').notNull().default('idle'),
	/** the build string from GET /v1/capabilities, e.g. ++Wardogs+Live-CL-501228; '' until read */
	build: text('build').notNull().default(''),
	/** GET /v1/server-id on builds that serve it (CL-501228+): the join code; '' otherwise */
	gameServerId: text('game_server_id').notNull().default(''),
	/** when the game process started, from uptimeSeconds on GET /v1/health; null until read or unserved */
	startedAt: ts('started_at'),
	/** MaxReservedSlots from the config document: player slots held back for reserved players; null until read */
	reservedSlots: integer('reserved_slots'),
	/** Status as the action registry shapes it */
	status: jsonb('status'),
	/** Player[] as the action registry shapes it */
	players: jsonb('players'),
	playerCount: integer('player_count').notNull().default(0),
	statusAt: ts('status_at'),
	playersAt: ts('players_at'),
	/** last attempt, successful or not */
	observedAt: ts('observed_at'),
	/** when the last kill feed batch arrived (written by the web process that took it) */
	feedAt: ts('feed_at'),
	updatedAt: ts('updated_at').notNull().defaultNow()
});

/**
 * Trigger actions the rules decided on, written in the same transaction as the observation that
 * caused them and delivered by the worker afterwards. A crash between the two leaves the row, not
 * a lost whisper. `unknown` is a send with no answer; it is never retried automatically.
 */
export const outbox = pgTable(
	'outbox',
	{
		id: bigserial('id', { mode: 'number' }).primaryKey(),
		serverId: text('server_id')
			.notNull()
			.references(() => servers.id, { onDelete: 'cascade' }),
		triggerId: text('trigger_id'),
		triggerName: text('trigger_name').notNull().default(''),
		triggerKind: text('trigger_kind').notNull().default(''),
		/** an action name from the registry, or "sequence" with params.steps */
		action: text('action').notNull(),
		params: jsonb('params'),
		/** SteamID or message the audit row names */
		target: text('target').notNull().default(''),
		/** extra fields for the audit row */
		detail: jsonb('detail'),
		/** a whisper or kick is only meaningful while the player is on; null for broadcasts */
		steamId: text('steam_id'),
		okMessage: text('ok_message').notNull().default(''),
		dedupeKey: text('dedupe_key').notNull(),
		/** pending | delivered | failed | unknown | skipped */
		state: text('state').notNull().default('pending'),
		attempts: integer('attempts').notNull().default(0),
		notBefore: ts('not_before').notNull().defaultNow(),
		leaseUntil: ts('lease_until'),
		outcome: text('outcome').notNull().default(''),
		createdAt: ts('created_at').notNull().defaultNow(),
		doneAt: ts('done_at')
	},
	(t) => [
		uniqueIndex('outbox_dedupe_idx').on(t.dedupeKey),
		index('outbox_pending_idx').on(t.state, t.notBefore),
		index('outbox_server_idx').on(t.serverId, t.createdAt.desc())
	]
);

/**
 * Hourly rollups of samples (rollups.ts fills them; analytics.ts reads them for the long ranges).
 * Durations are seconds of cover; player_s is player-count × seconds while up. Kept for good.
 */
export const sampleRollups = pgTable(
	'sample_rollups',
	{
		serverId: text('server_id').notNull(),
		bucket: ts('bucket').notNull(),
		samples: integer('samples').notNull().default(0),
		okSamples: integer('ok_samples').notNull().default(0),
		upS: real('up_s').notNull().default(0),
		downS: real('down_s').notNull().default(0),
		playerS: real('player_s').notNull().default(0),
		maxPlayers: integer('max_players'),
		maxCap: integer('max_cap')
	},
	(t) => [primaryKey({ columns: [t.serverId, t.bucket] })]
);

export const sampleMapRollups = pgTable(
	'sample_map_rollups',
	{
		serverId: text('server_id').notNull(),
		bucket: ts('bucket').notNull(),
		map: text('map').notNull(),
		secs: real('secs').notNull().default(0)
	},
	(t) => [primaryKey({ columns: [t.serverId, t.bucket, t.map] })]
);

/** Owner-editable runtime settings (cadences, budgets, retention); see settings.ts for keys and bounds. */
export const siteSettings = pgTable('site_settings', {
	key: text('key').primaryKey(),
	value: jsonb('value'),
	updatedAt: ts('updated_at').notNull().defaultNow(),
	updatedBy: text('updated_by')
});

/**
 * One row: which worker process owns observation and delivery, with a lease it must keep
 * renewing. Every worker write checks the token inside its transaction (fencing), so a worker
 * that lost the lease can never write late.
 */
export const workerOwnership = pgTable('worker_ownership', {
	id: integer('id').primaryKey(),
	token: text('token').notNull(),
	label: text('label').notNull().default(''),
	acquiredAt: ts('acquired_at').notNull().defaultNow(),
	leaseUntil: ts('lease_until').notNull()
});

export type ServerRow = typeof servers.$inferSelect;
export type OrgRow = typeof organizations.$inferSelect;
export type OrgInviteRow = typeof orgInvites.$inferSelect;
export type OrgRoleRow = typeof orgRoles.$inferSelect;
export type ApiKeyRow = typeof apiKeys.$inferSelect;
export type AuditRow = typeof auditLog.$inferSelect;
export type SampleRow = typeof samples.$inferSelect;
export type SteamProfileRow = typeof steamProfiles.$inferSelect;
export type TriggerRow = typeof triggers.$inferSelect;
export type WebhookRow = typeof webhooks.$inferSelect;
export type PlayerNoteRow = typeof playerNotes.$inferSelect;
export type PlayerMarkRow = typeof playerMarks.$inferSelect;
export type ListRow = typeof lists.$inferSelect;
export type ListEntryRow = typeof listEntries.$inferSelect;
export type ServerListStateRow = typeof serverListState.$inferSelect;
export type ServerListSyncRow = typeof serverListSync.$inferSelect;
export type ServerLiveRow = typeof serverLive.$inferSelect;
export type OutboxRow = typeof outbox.$inferSelect;

/** Read-only administrator graphs; recorded once per 30 second bucket, scoped to one match. */
export const playerProgressSamples = pgTable(
	'player_progress_samples',
	{
		serverId: text('server_id').notNull(),
		matchId: bigint('match_id', { mode: 'number' })
			.notNull()
			.references(() => matches.id, { onDelete: 'cascade' }),
		bucket: bigint('bucket', { mode: 'number' }).notNull(),
		observedAt: ts('observed_at').notNull(),
		players: jsonb('players').notNull()
	},
	(t) => [
		primaryKey({ columns: [t.serverId, t.matchId, t.bucket] }),
		index('player_progress_match_idx').on(t.matchId, t.observedAt)
	]
);

export const factionLockRules = pgTable('faction_lock_rules', {
	serverId: text('server_id')
		.primaryKey()
		.references(() => servers.id, { onDelete: 'cascade' }),
	enabled: boolean('enabled').notNull().default(false),
	graceSeconds: integer('grace_seconds').notNull().default(120),
	capacities: jsonb('capacities').notNull().default({}),
	updatedAt: ts('updated_at').notNull().defaultNow()
});
export const factionMovePermits = pgTable(
	'faction_move_permits',
	{
		id: bigserial('id', { mode: 'number' }).primaryKey(),
		serverId: text('server_id').notNull(),
		steamId: text('steam_id').notNull(),
		faction: text('faction').notNull(),
		createdAt: ts('created_at').notNull().defaultNow(),
		expiresAt: ts('expires_at').notNull()
	},
	(t) => [index('faction_permit_lookup_idx').on(t.serverId, t.steamId, t.expiresAt)]
);
export const factionLockEvents = pgTable(
	'faction_lock_events',
	{
		id: bigserial('id', { mode: 'number' }).primaryKey(),
		serverId: text('server_id')
			.notNull()
			.references(() => servers.id, { onDelete: 'cascade' }),
		matchId: bigint('match_id', { mode: 'number' }).notNull(),
		steamId: text('steam_id').notNull(),
		fromFaction: text('from_faction').notNull(),
		toFaction: text('to_faction').notNull(),
		state: text('state').notNull(),
		reason: text('reason').notNull(),
		createdAt: ts('created_at').notNull().defaultNow(),
		updatedAt: ts('updated_at').notNull().defaultNow()
	},
	(t) => [index('faction_lock_pending_idx').on(t.serverId, t.state, t.createdAt)]
);

/** Per-organisation OpenAI-compatible assistant, isolated from enforcement. */
export const integrityAiSettings = pgTable('integrity_ai_settings', {
	autoEnabled: boolean('auto_enabled').notNull().default(true),
	dailyLimit: integer('daily_limit').notNull().default(100),
	budgetDay: text('budget_day').notNull().default(''),
	dailyRequests: integer('daily_requests').notNull().default(0),
	orgId: text('org_id')
		.primaryKey()
		.references(() => organizations.id, { onDelete: 'cascade' }),
	baseUrl: text('base_url').notNull(),
	model: text('model').notNull(),
	keyEnc: text('key_enc').notNull(),
	tokenParameter: text('token_parameter').notNull().default('max_tokens'),
	maxTokens: integer('max_tokens').notNull().default(1200),
	updatedAt: ts('updated_at').notNull().defaultNow(),
	lastRequestAt: ts('last_request_at')
});
export const integrityAiReviews = pgTable('integrity_ai_reviews', {
	fingerprint: text('fingerprint').primaryKey(),
	caseId: text('case_id')
		.notNull()
		.references(() => integrityCases.id, { onDelete: 'cascade' }),
	result: jsonb('result').notNull(),
	createdAt: ts('created_at').notNull().defaultNow()
});

export const weaponRestrictionRules = pgTable('weapon_restriction_rules', {
	serverId: text('server_id')
		.primaryKey()
		.references(() => servers.id, { onDelete: 'cascade' }),
	enabled: boolean('enabled').notNull().default(false),
	causes: jsonb('causes').notNull().default([]),
	groups: jsonb('groups').notNull().default([]),
	updatedAt: ts('updated_at').notNull().defaultNow()
});
export const weaponRestrictionEvents = pgTable(
	'weapon_restriction_events',
	{
		id: text('id').primaryKey(),
		serverId: text('server_id')
			.notNull()
			.references(() => servers.id, { onDelete: 'cascade' }),
		matchId: bigint('match_id', { mode: 'number' })
			.notNull()
			.references(() => matches.id, { onDelete: 'cascade' }),
		ruleVersion: text('rule_version').notNull(),
		steamId: text('steam_id').notNull(),
		playerName: text('player_name').notNull(),
		cause: text('cause').notNull(),
		eventId: text('event_id').notNull(),
		action: text('action').notNull(),
		state: text('state').notNull(),
		reason: text('reason').notNull(),
		clock: real('clock').notNull(),
		createdAt: ts('created_at').notNull(),
		updatedAt: ts('updated_at').notNull()
	},
	(t) => [
		index('weapon_restriction_player_idx').on(t.serverId, t.matchId, t.steamId, t.createdAt.desc())
	]
);

export const integrityAiJobs = pgTable(
	'integrity_ai_jobs',
	{
		caseId: text('case_id')
			.primaryKey()
			.references(() => integrityCases.id, { onDelete: 'cascade' }),
		state: text('state').notNull().default('pending'),
		attempts: integer('attempts').notNull().default(0),
		nextAt: ts('next_at').notNull().defaultNow(),
		leaseUntil: ts('lease_until'),
		claimToken: text('claim_token'),
		lastError: text('last_error'),
		result: jsonb('result'),
		updatedAt: ts('updated_at').notNull().defaultNow()
	},
	(t) => [index('integrity_ai_jobs_due_idx').on(t.state, t.nextAt)]
);

export const skillBalanceRules = pgTable('skill_balance_rules', {
	serverId: text('server_id')
		.primaryKey()
		.references(() => servers.id, { onDelete: 'cascade' }),
	enabled: boolean('enabled').notNull().default(false),
	graceSeconds: integer('grace_seconds').notNull().default(300),
	leadPoints: integer('lead_points').notNull().default(40),
	updatedAt: ts('updated_at').notNull().defaultNow()
});
export const skillBalanceRuns = pgTable(
	'skill_balance_runs',
	{
		id: text('id').primaryKey(),
		serverId: text('server_id')
			.notNull()
			.references(() => servers.id, { onDelete: 'cascade' }),
		matchId: bigint('match_id', { mode: 'number' }).notNull(),
		state: text('state').notNull(),
		reason: text('reason').notNull(),
		plan: jsonb('plan').notNull(),
		moves: jsonb('moves').notNull().default([]),
		createdAt: ts('created_at').notNull().defaultNow(),
		updatedAt: ts('updated_at').notNull().defaultNow()
	},
	(t) => [uniqueIndex('skill_balance_round_unique').on(t.serverId, t.matchId)]
);

export const numericLimitRules = pgTable('numeric_limit_rules', {
	serverId: text('server_id')
		.primaryKey()
		.references(() => servers.id, { onDelete: 'cascade' }),
	config: jsonb('config').notNull(),
	updatedAt: ts('updated_at').notNull().defaultNow()
});
export const numericLimitEvents = pgTable(
	'numeric_limit_events',
	{
		id: text('id').primaryKey(),
		serverId: text('server_id')
			.notNull()
			.references(() => servers.id, { onDelete: 'cascade' }),
		matchId: bigint('match_id', { mode: 'number' })
			.notNull()
			.references(() => matches.id, { onDelete: 'cascade' }),
		steamId: text('steam_id').notNull(),
		ruleVersion: text('rule_version').notNull(),
		action: text('action').notNull(),
		state: text('state').notNull(),
		evidence: jsonb('evidence').notNull(),
		createdAt: ts('created_at').notNull().defaultNow(),
		updatedAt: ts('updated_at').notNull().defaultNow()
	},
	(t) => [index('numeric_limit_player_idx').on(t.serverId, t.matchId, t.steamId, t.createdAt)]
);

export const groupControlRules = pgTable('group_control_rules', {
	serverId: text('server_id')
		.primaryKey()
		.references(() => servers.id, { onDelete: 'cascade' }),
	config: jsonb('config').notNull(),
	updatedAt: ts('updated_at').notNull().defaultNow()
});
export const groupControlScans = pgTable('group_control_scans', {
	serverId: text('server_id')
		.primaryKey()
		.references(() => servers.id, { onDelete: 'cascade' }),
	config: jsonb('config').notNull(),
	groups: jsonb('groups').notNull(),
	scannedAt: ts('scanned_at').notNull(),
	aiStatus: text('ai_status').notNull().default('not_requested'),
	aiResult: jsonb('ai_result'),
	aiFingerprint: text('ai_fingerprint')
});
