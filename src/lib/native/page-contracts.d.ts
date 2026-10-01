// Native page data contracts. Generated from pre-migration page return types.
import type { ServerInfo, Catalog, Features } from '$lib/types';
export interface PageContracts {
	'src/routes/(app)/+layout.server.ts': {
		user: import('$lib/server/access').SessionUser;
		orgs: import('$lib/server/access').OrgSummary[];
		scope: import('$lib/server/scope').Scope | null;
		canManage: boolean;
		canCreateOrg: boolean;
		servers: import('$lib/server/access').ServerSummary[];
		demoAllowed: boolean;
		enrolment: import('$lib/enrolment').EnrolmentStatus;
		authPolicy: import('$lib/server/enrolment').EnrolmentPolicy;
		steamLookup: boolean;
	};
	'src/routes/(app)/account/+page.server.ts': {
		sessions: import('$lib/server/users').SessionView[];
		discord: boolean;
		providers: string[];
		hasPassword: boolean;
		steamId: string;
		defaultOrgId: string;
		passkeys: import('$lib/server/users').PasskeyView[];
		methods: import('$lib/enrolment').AuthMethods;
		enrolment: import('$lib/enrolment').Enrolment;
		status: import('$lib/enrolment').EnrolmentStatus;
		policy: import('$lib/server/enrolment').EnrolmentPolicy;
		recoveryKeyAt: string | null;
	};
	'src/routes/(app)/admin/+layout.server.ts': {};
	'src/routes/(app)/admin/+page.server.ts': { overview: import('$lib/server/overview').Overview };
	'src/routes/(app)/admin/qq/+page.server.ts': {
		config: {
			revision: string;
			source: string;
			provider: 'official' | 'napcat' | 'llbot';
			enabled: boolean;
			url: string;
			selfId: string;
			hasToken: boolean;
			hasSecret: boolean;
			policies: {
				enabled: boolean;
				antiCheatNotices: boolean;
				serverId: string;
				groups: string[];
				lowAt: number;
				pointsPerMinute: number;
				voteCost: number;
				broadcastCost: number;
				reserveCost: number;
				reserveHours: number;
				voteSeconds: number;
				maps: string[];
			}[];
			updatedAt: string | null;
		};
		vips: import('$lib/server/qq/vip').VipSettings;
		servers: { id: string; name: string; orgName: string }[];
		callback: string;
	};
	'src/routes/(app)/admin/settings/+page.server.ts': {
		settings: import('$lib/server/settings').SettingView[];
	};
	'src/routes/(app)/admin/users/+page.server.ts': {
		users: import('$lib/types').UserView[];
		servers: import('$lib/server/access').ServerSummary[];
		rolesByOrgId: Record<string, { id: string; name: string; builtin: string | null }[]>;
	};
	'src/routes/(app)/audit/+page.server.ts': {
		entries: {
			id: number;
			userAgent: string;
			orgId: string | null;
			serverId: string | null;
			ts: Date;
			actorId: string | null;
			actorName: string;
			serverName: string;
			category: string;
			action: string;
			target: string;
			detail: unknown;
			outcome: 'ok' | 'error' | 'denied';
			status: number | null;
			message: string;
			durationMs: number | null;
		}[];
		nextBefore: number | null;
		filters: {
			serverId: string | undefined;
			actorId: string | undefined;
			category: string | undefined;
			action: string | undefined;
			outcome: string | undefined;
			q: string | undefined;
			from: string | undefined;
			to: string | undefined;
			before: number | undefined;
			limit: number;
		};
		actions: { category: string; action: string }[];
		actors: { actorId: string; actorName: string }[];
	};
	'src/routes/(app)/orgs/+page.server.ts': {
		orgViews: {
			role: import('$lib/server/access').OrgRole;
			listKinds: import('$lib/types').ListKind[];
			id: string;
			name: string;
			slug: string;
			memberCount: number;
			serverCount: number;
			serverLimit: number;
			customServerLimit: number | null;
			suspended: { at: string; reason: string } | null;
			allowPublicStatus: boolean;
			allowPublicLeaderboards: boolean;
			discordInviteUrl: string;
			createdBy: { username: string; name: string } | null;
			createdAt: string | null;
		}[];
	};
	'src/routes/(app)/orgs/[id]/+layout.server.ts': {
		org: import('$lib/types').OrgView;
		orgServers: import('$lib/server/access').ServerSummary[];
		listsRole: import('$lib/server/access').ListsRole;
		lists: import('$lib/types').OrgListsView;
	};
	'src/routes/(app)/orgs/[id]/+page.server.ts': {
		members: import('$lib/types').OrgMemberView[];
		invites: import('$lib/types').InviteView[];
		webhooks: import('$lib/types').WebhookView[];
		roles: import('$lib/types').RoleView[];
		keys: import('$lib/types').ApiKeyView[];
		webhookEvents: { key: string; label: string }[];
		https: boolean;
		discord: boolean;
	};
	'src/routes/(app)/orgs/[id]/access/+page.server.ts': {
		members: import('$lib/types').OrgMemberView[];
		roles: import('$lib/types').RoleView[];
	};
	'src/routes/(app)/orgs/[id]/bans/+page.server.ts': {
		entries: import('$lib/types').ListEntryView[];
	};
	'src/routes/(app)/orgs/[id]/integrity/+page.server.ts': {
		modelConfig: {
			hasToken: boolean;
			modelId: string;
			stage: string;
			bucketSeconds: number;
			windowSeconds: number;
			windowSteps: number;
			referenceSamples: number;
			p95: number;
			p97: number;
			p98: number;
			p99: number;
			revision: string;
			developerEnabled: boolean;
			url: string;
			autoPunishEnabled: boolean;
			maxActionsPerHour: number;
			cooldownSeconds: number;
			intervalSeconds: number;
		};
		modelRuns: Record<string, any>[];
		orgId: string;
		ruleVersion: number;
		assessmentMode: import('$lib/server/integrity/statistics').AssessmentMode;
		comparison: {
			since: string | null;
			total: number;
			normalNormal: number;
			normalAbnormal: number;
			abnormalNormal: number;
			abnormalAbnormal: number;
		};
		imports: {
			firstEventAt: string;
			lastEventAt: string;
			stagedAt: string;
			reviewedAt: string | null;
			id: string;
			orgId: string;
			sourceServer: string;
			fileSha256: string;
			status: string;
			rowCount: number;
			stagedBy: string;
			reviewedBy: string | null;
		}[];
		baselineSummary: {
			metrics: number;
			externalMetrics: number;
			maxSamples: number;
			calculatedAt: string;
		};
		ruleConfig: import('$lib/server/integrity/score').IntegrityRuleConfig;
		ruleDefaults: import('$lib/server/integrity/score').IntegrityRuleConfig;
		enforcement: {
			autoSuspendedAt: string | null;
			autoKickEnabled: boolean;
			autoQuarantine24hEnabled: boolean;
			autoQuarantine7dEnabled: boolean;
			autoActionMaxPerHour: number;
			autoActionMaxPercentOnline: number;
		};
		weaponOverrides: { cause: string; category: string }[];
		weaponDefaults: Readonly<
			Record<
				string,
				| 'INFANTRY'
				| 'VEHICLE'
				| 'MORTAR'
				| 'ARTILLERY'
				| 'FIXED_AA'
				| 'CIWS'
				| 'FIXED_WEAPON'
				| 'ROADKILL'
				| 'ENVIRONMENT'
				| 'SUICIDE'
				| 'UNKNOWN'
			>
		>;
		weaponCategories: readonly [
			'INFANTRY',
			'VEHICLE',
			'MORTAR',
			'ARTILLERY',
			'FIXED_AA',
			'CIWS',
			'FIXED_WEAPON',
			'ROADKILL',
			'ENVIRONMENT',
			'SUICIDE',
			'UNKNOWN'
		];
	};
	'src/routes/(app)/orgs/[id]/players/+page.server.ts': {
		players: import('$lib/server/seen').SeenPlayer[];
		total: number;
		filters: import('$lib/server/seen').SeenFilters;
		pageSize: number;
	};
	'src/routes/(app)/orgs/[id]/reserved/+page.server.ts': {
		entries: import('$lib/types').ListEntryView[];
	};
	'src/routes/(app)/orgs/[id]/roles/+page.server.ts': { roles: import('$lib/types').RoleView[] };
	'src/routes/(app)/plugins/+page.server.ts': {
		plugins: import('$lib/plugins/sdk').PersonalPlugin[];
		catalog: {
			apiVersion: 1;
			id: string;
			name: string;
			description: string;
			version: string;
			renderer: string;
			style: { accent: string; columns: number; density: 'compact' | 'comfortable' };
			widgets: (
				| {
						type: 'metric';
						title: string;
						metric: 'cash' | 'kills' | 'deaths' | 'online' | 'averagePing';
				  }
				| { type: 'text'; title: string; text: string }
				| {
						type: 'players';
						title: string;
						columns: ('name' | 'steamId' | 'cash' | 'faction' | 'kills' | 'deaths' | 'ping')[];
						limit: number;
						sortBy: 'cash' | 'kills' | 'deaths' | 'ping';
				  }
			)[];
		}[];
		pluginServers: { id: string; name: string; orgName: string }[];
	};
	'src/routes/(app)/plugins/[pluginId]/+page.server.ts': {
		plugin: import('$lib/plugins/sdk').PersonalPlugin;
	};
	'src/routes/(app)/qq-link/+page.server.ts': { links: Record<string, any>[] };
	'src/routes/(app)/server/[id]/+layout.server.ts': {
		server: import('$lib/types').ServerInfo;
		catalog: import('$lib/types').Catalog;
		features: import('$lib/types').Features;
		reachable: boolean;
		problem: string;
		identity: { build: string; gameServerId: string; startedAt: string | null };
	};
	'src/routes/(app)/server/[id]/automation/+page.server.ts': {
		factionQuota: { config: import('$lib/server/faction-quota').QuotaConfig; runtime: Runtime };
		factionScores: import('$lib/types').FactionScore[];
		groupControl: {
			config: {
				mode: 'manual' | 'off' | 'auto';
				engine: 'structured' | 'ai';
				prefixLength: number;
				similarityPercent: number;
				minPlayers: number;
			};
			scan: {
				groups: import('$lib/group-control-policy').SuspectedGroup[];
				config: {
					mode: 'manual' | 'off' | 'auto';
					engine: 'structured' | 'ai';
					prefixLength: number;
					similarityPercent: number;
					minPlayers: number;
				};
				aiResult: {
					groups: {
						id: string;
						assessment: 'possible_group' | 'uncertain' | 'likely_coincidence';
						reason: string;
					}[];
				} | null;
				scannedAt: string;
				serverId: string;
				aiStatus: string;
				aiFingerprint: string | null;
			} | null;
		};
		numericLimits: {
			config: import('$lib/numeric-limit-policy').NumericLimits;
			events: {
				createdAt: string;
				updatedAt: string;
				id: string;
				serverId: string;
				matchId: number;
				steamId: string;
				ruleVersion: string;
				action: string;
				state: string;
				evidence: unknown;
			}[];
		};
		skillBalance: {
			executionAvailable: boolean;
			executionBlock: string;
			rule: {
				serverId: string;
				enabled: boolean;
				graceSeconds: number;
				leadPoints: number;
				updatedAt: Date;
			};
			runs: {
				plan: import('$lib/skill-balance-policy').BalancePlan;
				moves: import('$lib/server/skill-balance').BalanceMove[];
				createdAt: string;
				updatedAt: string;
				state: string;
				id: string;
				serverId: string;
				matchId: number;
				reason: string;
			}[];
		};
		factionLock: {
			rule: {
				serverId: string;
				enabled: boolean;
				graceSeconds: number;
				capacities: unknown;
				updatedAt: Date;
			};
			events: {
				createdAt: string;
				updatedAt: string;
				id: number;
				serverId: string;
				matchId: number;
				steamId: string;
				fromFaction: string;
				toFaction: string;
				state: string;
				reason: string;
			}[];
		};
		weaponRestriction: {
			rule: { enabled: boolean; causes: string[]; groups: string[] };
			events: {
				createdAt: string;
				updatedAt: string;
				id: string;
				serverId: string;
				matchId: number;
				ruleVersion: string;
				steamId: string;
				playerName: string;
				cause: string;
				eventId: string;
				action: string;
				state: string;
				reason: string;
				clock: number;
			}[];
			catalogue: { cause: string; label: string }[];
			catalogueTruncated: boolean;
		};
		teams: string[];
		triggers: import('$lib/types').TriggerView[];
		steam: boolean;
		feed: boolean;
	};
	'src/routes/(app)/server/[id]/bans/+page.server.ts': {
		listState: import('$lib/types').ServerListsState;
		orgLists: import('$lib/types').OrgListsView | null;
	};
	'src/routes/(app)/server/[id]/discord/+page.server.ts': never;
	'src/routes/(app)/server/[id]/faction-lock/+page.server.ts': {
		teams: string[];
		rule: {
			serverId: string;
			enabled: boolean;
			graceSeconds: number;
			capacities: unknown;
			updatedAt: Date;
		};
		events: {
			createdAt: string;
			updatedAt: string;
			id: number;
			serverId: string;
			matchId: number;
			steamId: string;
			fromFaction: string;
			toFaction: string;
			state: string;
			reason: string;
		}[];
	};
	'src/routes/(app)/server/[id]/integrity/+page.server.ts': {
		longModelResults: Record<string, any>[];
		distributions: {
			status: string;
			updatedAt: string | null;
			lastFailureAt: string | null;
			refreshMinutes: 5;
			dataBefore: string | null;
			metrics: import('$lib/integrity-distribution').DistributionChartMetric[];
		};
		shortRisk: {
			available: boolean;
			evaluatedAt: string | null;
			autoKick: boolean;
			warningPercentile: number;
			kickPercentile: number;
			players: {
				level: string;
				server_id: string;
				player_id: string;
				name?: string | undefined;
				ShortRisk: number | null;
				anomaly_score?: number | undefined;
				percentile?: number | undefined;
				scope?: string[] | undefined;
				status: string;
				recent_windows?: import('$lib/short-risk-policy').ShortWindow[] | undefined;
			}[];
			history: RecordValue[];
		} | null;
		aiJobs: {
			caseId: string;
			state: string;
			attempts: number;
			lastError: string | null;
			updatedAt: string;
			result: {
				verdict: '建议通过' | '建议复核' | '证据不足';
				suspicionPercent: number | null;
				evidenceQuality: '低' | '中' | '高';
				alternatives: string[];
				summary: string;
				reasons: { text: string; evidence: string }[];
				contradictions: string[];
				missingEvidence: string[];
			} | null;
			disposition: string | null;
			promptVersion: string;
		}[];
		aiAutoEnabled: boolean;
		reviewPenalties: {
			caseId: string;
			id: string;
			deliveryState: string | null;
			expiresAt: string | null;
			active: boolean;
		}[];
		cases: {
			createdAt: string;
			name: string | null;
			id: string;
			steamId: string;
			status: string;
			confidence: string;
			riskScore: number;
			riskBreakdown: unknown;
			statistical: unknown;
			snapshot: unknown;
			trigger: string;
		}[];
		scores: {
			scoredAt: string;
			id: number;
			steamId: string;
			score: number;
			level: string;
			breakdown: unknown;
			statistical: unknown;
			ruleVersion: number;
		}[];
		reports: {
			createdAt: string;
			id: number;
			targetSteamId: string;
			reason: string;
			status: string;
		}[];
		actions: {
			id: any;
			steamId: any;
			caseId: any;
			source: any;
			action: any;
			deliveryState: any;
			deliveryReason: any;
			name: any;
			score: any;
			threshold: any;
			percentileLabel: string;
			createdAt: string;
			effectiveAt: string | null;
			expiresAt: string | null;
			revertedAt: string | null;
		}[];
		actionsPagination: {
			page: number;
			pages: number;
			total: any;
			pageSize: number;
			filter: import('$lib/server/integrity/action-history').ActionFilter;
			before: string;
		};
		historyPolicy: import('$lib/integrity-retention').HistoryRetentionPolicy;
		historyPolicyRevision: string;
		historyLastCleanup: { at: string; removed: number } | null;
		feedAt: string | null;
		feedConfigured: boolean;
		playersAt: string | null;
		feedRowsTruncated: boolean;
		feedMetricsAvailable: boolean;
		onlinePlayers: {
			steamId: string;
			name: string;
			kills: number;
			deaths: number;
			infantry: import('$lib/server/integrity/live').LiveInfantryMetrics | null;
			riskScore: any;
			riskLevel: any;
			riskBreakdown: any;
		}[];
		ruleVersion: number;
		assessmentMode: import('$lib/server/integrity/statistics').AssessmentMode;
		comparison: {
			since: string | null;
			total: number;
			normalNormal: number;
			normalAbnormal: number;
			abnormalNormal: number;
			abnormalAbnormal: number;
		};
		committeeShadow: {
			modelVersion: 'ensemble-server-round-v3';
			votingVersion: string;
			counts: Record<import('$lib/server/integrity/committee').CommitteeDecision, number>;
			models: Record<
				string,
				Record<import('$lib/server/integrity/committee').ExpertDecision, number>
			>;
			unknownReasons: Record<string, Record<string, number>>;
			assessed: number;
			disagreement: number;
			disagreementRate: number | null;
			confirmedAbuse: number;
			falsePositive: number;
			truncated: boolean;
		};
		labels: { caseId: string; label: string; reason: string; createdAt: string }[];
		kpmBands: { min: number; points: number }[];
		mode: string;
		canConfigure: boolean;
		orgIntegrityUrl: string | null;
	};
	'src/routes/(app)/server/[id]/integrity/ai/+page.server.ts': {
		jobs: {
			caseId: string;
			state: string;
			attempts: number;
			lastError: string | null;
			updatedAt: string;
			result: {
				verdict: '建议通过' | '建议复核' | '证据不足';
				suspicionPercent: number | null;
				evidenceQuality: '低' | '中' | '高';
				alternatives: string[];
				summary: string;
				reasons: { text: string; evidence: string }[];
				contradictions: string[];
				missingEvidence: string[];
			} | null;
			disposition: string | null;
			promptVersion: string;
		}[];
		prompt: string;
		promptVersion: string;
		canConfigure: boolean;
		config: {
			autoEnabled: boolean;
			autoCloseEnabled: boolean;
			deleteLowRisk: boolean;
			dailyLimit: number;
			dailyRequests: number;
			baseUrl: string;
			model: string;
			maxTokens: number;
			tokenParameter: string;
			hasKey: boolean;
		} | null;
		cases: { name: string | null; id: string; steamId: string; createdAt: Date; status: string }[];
	};
	'src/routes/(app)/server/[id]/integrity/cases/+page.server.ts': {
		view: string;
		page: number;
		pages: number;
		pageSize: number;
		total: number;
		before: string;
		cases: {
			createdAt: string;
			name: string | null;
			id: string;
			steamId: string;
			status: string;
			confidence: string;
			riskScore: number;
			riskBreakdown: unknown;
			statistical: unknown;
			snapshot: unknown;
			trigger: string;
		}[];
		labels: { caseId: string; label: string; reason: string; createdAt: string }[];
		aiJobs: {
			caseId: string;
			state: string;
			attempts: number;
			lastError: string | null;
			updatedAt: string;
			result: {
				verdict: '建议通过' | '建议复核' | '证据不足';
				suspicionPercent: number | null;
				evidenceQuality: '低' | '中' | '高';
				alternatives: string[];
				summary: string;
				reasons: { text: string; evidence: string }[];
				contradictions: string[];
				missingEvidence: string[];
			} | null;
			disposition: string | null;
			promptVersion: string;
		}[];
		aiAutoEnabled: boolean;
		canConfigure: boolean;
		reviewPenalties: {
			caseId: string;
			id: string;
			deliveryState: string | null;
			expiresAt: string | null;
			active: boolean;
		}[];
	};
	'src/routes/(app)/server/[id]/matches/[matchId]/+page.server.ts': {
		match: import('$lib/matches').MatchView;
	};
	'src/routes/(app)/server/[id]/players/[steamId]/+page.server.ts': {
		playerProgress:
			| {
					id: number;
					map: string;
					startedAt: string;
					endedAt: string | null;
					truncated: boolean;
					points: import('$lib/player-progress').CashPoint[];
			  }[]
			| null;
		dossier: import('$lib/types').DossierView;
		career: import('$lib/leaderboard').CareerView;
		multiServer: boolean;
		integrity: {
			aliases: string[];
			firstSeen: string;
			lastSeen: string;
			riskScore: number | null;
			riskLevel: string | null;
			breakdown: import('$lib/server/integrity/score').RiskComponent[];
			latestWindow: {
				kpm180: number;
				infantryKills: number;
				uniqueVictims: number;
				observedAt: string;
			} | null;
			reports24h: number;
			current: { kills: number; deaths: number } | null;
			metrics: import('$lib/server/integrity/live').LiveInfantryMetrics | null;
			metricsAvailable: boolean;
		} | null;
		weaponDistances: {
			days: number;
			minimumSamples: number;
			refreshedAt: string;
			rows: {
				distribution: {
					upper: number;
					step: number;
					players: number;
					peak: number;
					bins: { from: number; to: number; center: number; count: number }[];
				};
				steamId: string;
				cause: string;
				samples: number;
				average: number;
				maximum: number;
				serverAverage: number;
				eligiblePlayers: number;
				rank: number | null;
			}[];
		} | null;
	};
	'src/routes/(app)/server/[id]/public/+page.server.ts': never;
	'src/routes/(app)/server/[id]/qq-bindings/+page.server.ts': {
		bindings: {
			links: Record<string, any>[];
			recent: Record<string, any>[];
			total: number;
			page: number;
			query: string;
		};
		canManage: boolean;
	};
	'src/routes/(app)/server/[id]/settings/+page.server.ts': {
		owner: boolean;
		https: boolean;
		origin: string;
		channels: import('$lib/types').WebhookView[];
	};
	'src/routes/(app)/server/[id]/slots/+page.server.ts': {
		listState: import('$lib/types').ServerListsState;
		orgLists: import('$lib/types').OrgListsView | null;
	};
	'src/routes/(app)/servers/+page.server.ts': {
		managed: import('$lib/server/access').ServerSummary[];
		ownedOrgs: import('$lib/server/access').OrgSummary[];
	};
	'src/routes/(app)/settings/+page.server.ts': never;
	'src/routes/(app)/users/+page.server.ts': never;
	'src/routes/(auth)/join/[token]/+page.server.ts':
		| {
				valid: false;
				problem: string;
				discord: boolean;
				org?: undefined;
				orgRole?: undefined;
				serverRole?: undefined;
				alreadyMember?: undefined;
				turnstileSiteKey?: undefined;
		  }
		| {
				valid: boolean;
				problem: string | null;
				org: { id: string; name: string };
				orgRole: 'owner' | 'member';
				serverRole: string | null;
				alreadyMember: boolean;
				discord: boolean;
				turnstileSiteKey: string | null;
		  };
	'src/routes/(auth)/recover/+page.server.ts': {};
	'src/routes/(auth)/setup/+page.server.ts': { tokenRequired: boolean };
	'src/routes/(auth)/sign-in/+page.server.ts': {
		discord: boolean;
		next: string;
		orgSignup: boolean;
	};
	'src/routes/(auth)/sign-in/verify/+page.server.ts': { next: string };
	'src/routes/(auth)/sign-up/+page.server.ts': {
		discord: boolean;
		turnstileSiteKey: string | null;
		remaining: number | null;
	};
	'src/routes/(public)/+layout.server.ts': {};
	'src/routes/(public)/s/[id]/+page.server.ts': {
		view: import('$lib/server/public').PublicStatus;
		heading: {
			id: string;
			name: string;
			orgName: string;
			discordInviteUrl: string;
			features: import('$lib/features').FeatureSet;
		};
	};
	'src/routes/(public)/s/[id]/leaderboard/+page.server.ts': {
		board: {
			maxPage: number;
			query: import('$lib/leaderboard').BoardQuery;
			rows: import('$lib/leaderboard').BoardRow[];
			total: number;
			pageSize: number;
			hasFeed: boolean;
		};
		orgScope: boolean;
		heading: {
			id: string;
			name: string;
			orgName: string;
			discordInviteUrl: string;
			features: import('$lib/features').FeatureSet;
		};
	};
	'src/routes/(public)/s/[id]/matches/+page.server.ts': {
		list: import('$lib/matches').MatchListView;
		heading: {
			id: string;
			name: string;
			orgName: string;
			discordInviteUrl: string;
			features: import('$lib/features').FeatureSet;
		};
	};
	'src/routes/(public)/s/[id]/matches/[matchId]/+page.server.ts': {
		match: import('$lib/matches').MatchView;
		feed: import('$lib/server/public').PublicKill[];
		more: boolean;
		heading: {
			id: string;
			name: string;
			orgName: string;
			discordInviteUrl: string;
			features: import('$lib/features').FeatureSet;
		};
	};
	'src/routes/(public)/s/[id]/players/[steamId]/+page.server.ts': {
		career: import('$lib/leaderboard').CareerView;
		combat: import('$lib/types').CombatSummary | null;
		player: { steamId: string; name: string; avatar: string };
		multiServer: boolean;
		heading: {
			id: string;
			name: string;
			orgName: string;
			discordInviteUrl: string;
			features: import('$lib/features').FeatureSet;
		};
	};
	'src/routes/(public)/s/[id]/report/+page.server.ts': {
		heading: {
			id: string;
			name: string;
			orgName: string;
			discordInviteUrl: string;
			features: import('$lib/features').FeatureSet;
		};
		signedIn: boolean;
	};
	'src/routes/+layout.server.ts': {
		user: import('$lib/server/access').SessionUser | null;
		appName: string;
	};
}
