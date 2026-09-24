# WARDOGS Community Integrity System Technical Implementation Plan

审计基线：`1508425541zhang/warcon--China` 的 `main`，`8c6707e78c88270333d5f90c89346536a6055498`（2026-09-23）。核对上游 `warcon-app/warcon` 的 HEAD 为同一提交。本文是 Phase 0 的设计交付，不启用自动处罚，不修改现有 RCON 实现。以下路径均相对仓库根目录。

## A. Warcon 现有架构

- Bun + SvelteKit + Drizzle + PostgreSQL/可选 TimescaleDB。`src/hooks.server.ts` 按 `WARCON_ROLE` 启动；`src/worker/worker.ts`、`src/worker/runtime.ts` 运行 worker；`docker-compose.yml` 分为 migrate、web、worker、db。`src/lib/server/db/schema.ts` 是 schema 源，`drizzle/0000_init.sql` 至 `drizzle/0033_org_list_capabilities.sql` 为迁移。
- RCON：`src/lib/server/rcon.ts`、`transport.ts` 持有现成 WARDOGS 客户端；`actions.ts` 定义命令和能力；`rcon-run.ts` 写审计；`dispatcher.ts` 为每台服务器串行派发。协议边界见 `docs/wardogs-api.md`。本项目不重写或逆向这些模块。
- Worker：`poller.ts` 排程，`observe.ts` 取状态和玩家，`sessions.ts` 持久化在线会话；`leadership.ts` 提供租约和 fencing；`outbox.ts` 负责持久化、去重和递送，未知结果不盲目重试。
- Kill Feed：`src/routes/api/ingest/events/+server.ts` 验证专用 bearer、限制大小和速率；`feed-core.ts` 解析已验证的 `killed` 字段；`feed.ts` 批量入库、按事件 ID 去重并补充会话侧阵营；`gateway*.ts` 把已入库事件交给 `feed-events.ts`，后者驱动现有 Kill Rate/Team Kill 规则。`db/schema.ts` 中的 `kills` 表保存原始 cause、标签、距离、双方 SteamID、接收时间和游戏内 `eventTime`。
- UI：`src/routes/(app)/server/[id]/` 有玩家、击杀、自动化、分析、档案；`src/routes/(app)/orgs/[id]/` 有组织封禁、权限与成员；`src/routes/(app)/audit/` 有审计。API 位于 `src/routes/api/`。`src/lib/capabilities.ts` 与 `src/lib/server/access.ts` 共同管理 RBAC。

## B. 可直接复用

| 能力 | 文件 | 使用方式 |
| --- | --- | --- |
| SteamID64 会话、别名、在线状态 | `src/lib/server/sessions.ts`、`players.ts`、`db/schema.ts` | 组织内档案以 `(org_id, steam_id)` 聚合，不以昵称作主键 |
| Steam persona、VAC/Game Ban 缓存 | `src/lib/server/steam.ts`、`steam_profiles` | 继续使用原 API key、缓存、退避；增加游戏时间时保留 UNKNOWN |
| 组织 watchlist/notes 与封禁同步 | `players.ts`、`lists.ts`、`lists-sync.ts`、`lists-plan.ts` | 复用可过期组织封禁及只撤销面板自身添加条目的语义 |
| Kill Feed、队列、审计、Discord | `feed*.ts`、`outbox.ts`、`audit.ts`、`webhook-delivery.ts` | 在已入库事件后处理；所有执行结果沿用现有审计/出站机制 |
| 定时广播、规则 dry run | `trigger-rules.ts`、`triggers.ts`、`src/routes/(app)/server/[id]/automation/+page.svelte` | 广播可配置 180–300 秒；完整性历史回放另加，不冒充现有 dry run |

## C. 需要调整的现有功能

1. `src/lib/server/risk.ts` 的现有 *advisory* 评分与目标规则冲突：私密资料和私密好友列表会加分，单次 VAC 权重可很高，`triggers.ts` 的 `risk_kick` 可按该分数踢人。保留现有评分供旧规则兼容，新建版本化 Integrity 评分，不复用旧分数作 54/64 自动处罚输入；UI 明确区分两种分数。目标系统不因 Private 加分，Steam 处罚先验总分默认封顶 15。
2. `src/lib/server/kill-rate.ts` 目前按 `causeKind === 'weapon'` 识别，窗口默认 5 分钟，目标则是严格可配置的 180 秒步兵窗口。保留旧告警规则；新检测器独立使用分类映射，UNKNOWN、迫击炮、车辆、固定武器、环境、自杀、队友击杀均不计入。
3. `feed.ts` 的 `ts` 是接收时间；`eventTime` 是每局钟表且 `matchId` 实测可能跨换图不变。新检测器必须用 `instanceId`、地图/局边界和同批相对时间处理乱序；钟表回退或跨度异常降级证据等级。阵营来自会话快照，不是事件原生字段，阵营缺失时不得断言“非队友击杀”。
4. `outbox.ts` 当前处理 trigger intent；Integrity 执行须增加独立、可恢复的 action/case 关联及动作前再校验，在 `unknown` 状态不重复下发。`lists-sync.ts` 需要识别处罚来源和审核状态，避免自动到期移除管理员独立封禁。

## D. 新增服务与边界

建议新增 `src/lib/server/integrity/`：`weapons.ts`（版本化武器分类）、`windows.ts`（滚动窗口/独立窗口）、`score.ts`（可解释风险与衰减）、`confidence.ts`（A/B/C/D）、`evidence.ts`（冻结快照）、`reports.ts`（去重和举报者可靠度）、`decisions.ts`（Dry Run/KO/隔离状态机）、`review.ts`（复核）、`replay.ts`（历史回放）。规则以组织为作用域，服务器可覆盖受控参数；每次判定保存规则版本和参数快照。只有当前服务器强行为异常且证据 A/B 才允许 365 天隔离；证据 C/D、DB 落盘失败、Feed 不健康时禁止长期隔离。自动系统永不产生永久封禁。

**聊天举报依赖**：`docs/wardogs-api.md` 只确认向玩家发送 `POST /v1/players/{steamId}/message`，Kill Feed 只确认 `killed` 事件；仓库没有经验证的玩家聊天接收端。故不得声称 `!report`/`!BAN` 已可从游戏聊天读取。先开放登录面板和有权限的机器人 API 举报，举报核心复用同一服务；若未来有官方、经授权且可验证的聊天事件入口，再增加适配器解析大小写不敏感的命令。聊天广播可先通过现有定时 `broadcast` 规则上线，但广播文案须说明实际可用的举报入口，避免引导玩家使用尚无法接收的命令。

## E. 数据库与 M. Migration 计划

在 `src/lib/server/db/schema.ts` 增加下列表，按阶段生成 `drizzle/0034_*.sql` 起的独立迁移并更新 `drizzle/meta/_journal.json`；具体序号以合并时上游最新迁移为准。现有 `kills`、`player_sessions`、`player_marks`、`player_notes`、`steam_profiles`、`list_entries` 不迁移为另一份事实表。

| 阶段 | 表/字段 | 约束和索引 |
| --- | --- | --- |
| 1 | `integrity_profiles`、`integrity_rule_sets` | `(org_id,steam_id)` 唯一；规则版本唯一、JSON 配置校验、创建者和审计引用 |
| 2–4 | `integrity_weapon_map`、`integrity_window_findings`、`integrity_risk_snapshots` | `(org_id,cause,version)`；窗口 `(org_id,server_id,steam_id,start_at)`；分项和来源事件 ID 持久化，独立窗口去重 |
| 5 | `integrity_cases`、`integrity_case_events`、`integrity_case_actions` | Case ID 唯一、事件按时间/玩家索引；快照不可变、哈希、保留期；动作 idempotency key 唯一 |
| 7 | `integrity_reports`、`integrity_reporter_stats` | 举报者/目标均 SteamID64；同一举报者对同一目标限流，唯一举报者按有效时间窗统计；误报只影响权重 |
| 11–14 | `integrity_sanctions`、`integrity_reviews`、`integrity_appeals` | 隔离/确认封禁/永久封禁分别标记；`starts_at`、`expires_at`、`case_id`、`risk_breakdown`、`review_status`；管理员复核和解除保留历史 |

迁移遵循先增后用：可空/新表 → 后台分批回填历史档案和分类 → 影子计算 → 灰度 Enforce。迁移前备份、在无 TimescaleDB 的 PostgreSQL 与 TimescaleDB 各运行一次，验证回滚为停用新服务和逆向数据迁移方案；绝不重写已有迁移。历史 kill 不因新规则自动处罚。每阶段一独立提交。

## F. Worker、G. API、H. UI

- Worker 接入点：`feed-events.ts` 在入库后将批次交给 Integrity 管道；`leadership.ts` fencing 保证单 owner 决策；`poller.ts` 调度过期解除、队列健康、跨服加入检查；`outbox.ts` 执行 KO/私聊/组织拒入；`dispatcher.ts` 保持人工命令优先。每事件只做内存窗口更新，数据库按批/状态转移写；Evidence 必须在长隔离前确认已持久化。
- 新 API 路由建议：`src/routes/api/orgs/[id]/integrity/{settings,cases,reviews,reports,appeals,weapon-map,replay}/+server.ts`，`src/routes/api/servers/[id]/integrity/{players,health}/+server.ts`，`src/routes/api/servers/[id]/players/[steamId]/integrity/+server.ts`。每个读取与动作按组织/服务器作用域校验，敏感操作记审计、限制速率；Case ID 查询不得越权。
- UI 接入：`src/routes/(app)/server/[id]/integrity/+page.svelte` 显示健康、在线风险和复核队列；`src/routes/(app)/server/[id]/players/[steamId]/+page.svelte` 增加当前分项、窗口、案件、处罚和历史；`src/routes/(app)/orgs/[id]/` 加组织复核/配置；`src/routes/(app)/server/[id]/+layout.svelte` 添加导航。中英文文案由新 `src/lib/i18n/` 资源提供，至少管理员完整性界面和玩家提示支持 English/简体中文；逐步汉化现有 UI，避免大规模替换导致上游合并困难。

## I. Discord、J. Steam、K. 权限

- 在 `webhook-delivery.ts` / `webhooks.ts` 增加 Integrity 事件类别和带 Case 链接、风险分项的 KO/隔离卡片；普通单次举报默认不通知。沿用加密 webhook URL、批处理和退避；通知故障不阻断行为判断，但应持久化重试状态和健康告警。
- `steam.ts` 增加官方公开 WARDOGS 游戏时间查询（先核实 App ID 和实际返回字段），在 `steam_profiles` 增加值、可见性和查询时间；Private/API 错误存 UNKNOWN，计 0 风险。保留 Steam API 的 24 小时缓存和退避。VAC/Game Ban 只作有限先验，按时间衰减。
- `capabilities.ts` 新增 `integrity.view`、`integrity.review`、`integrity.configure`、`integrity.enforce`、`integrity.appeal`；`access.ts`、`roles.ts`、API keys、角色 UI 同步。默认 viewer 只见授权范围的摘要，operator 可处理举报和查看案件，只有明确授予者可配置规则或转成确认/永久处罚。所有配置、复核和处罚进 `audit.ts`。

## L. 测试计划

沿用 `bun test`、`bun run check`、`bun run lint`、`scripts/ci.sh`、`scripts/smoke.sh`。新纯函数测试覆盖武器 UNKNOWN、迫击炮/车辆排除、精确 180 秒边界、同批乱序/跨局/重复事件、15 分钟独立窗口、unique victims、头部/穿透样本门槛、风险衰减及封顶。DB 集成测试覆盖租约切换、Evidence 与决策原子性、重启恢复、过期解除不误删人工封禁、权限越权和审计。场景测试包括 20 人恶意举报正常高手不长期隔离、正常高手高 KD 不长期隔离、Rage Cheat 在 A/B 证据下 KO/隔离、Feed/Steam/Discord/DB 故障的降级行为。每一阶段运行相关测试、检查迁移/RBAC/审计和现有回归。

## N. 性能风险、O. 安全风险、P. 上游兼容风险

- 性能：100+ 玩家和多服情况下，避免每 kill 查询 Steam/DB；内存按服务器与玩家限界、批量写风险快照；重启从近期 kills 恢复窗口时只作观察，不追溯执行；测量 feed 到动作延迟和 outbox 深度。现有 `kills` 在 TimescaleDB 可能压缩，Evidence 用独立不可变小快照，避免频繁回查全量历史。
- 安全：误判和恶意举报是最大风险；默认 DRY_RUN，禁止报告/Steam 历史独立触发长期隔离，操作前检查规则版本、Feed 健康、A/B 证据、Case 落盘和当前在线；可撤销并保留复核。SteamID64 严格校验，举报者身份只向授权管理员展示；不采集设备标识、IP 画像或客户端数据。现有 `risk_kick` 仍可能基于 Private/VAC 单独踢人，部署时需明确告知管理员并建议停用该旧规则，不能把旧动作计为 Integrity AUTO_KO。
- 上游兼容：新增独立 `integrity/` 服务、表、API 与 UI，尽量只在 `feed-events.ts`、`poller.ts`、`outbox.ts` 留小型接入点；不改 RCON/协议解析。上游若新增迁移，重编号本分支新增迁移并重新生成快照；保留旧规则和响应字段兼容。跨社区共享情报仅预留数据接口，不默认传播处罚。

## Q. Phase 0–15 实施顺序

| Phase | 交付与主要路径 |
| --- | --- |
| 0 | 本审计、基线测试和运行环境记录：本文件、`README.md`、`docs/wardogs-api.md` |
| 1 | schema/SteamID 档案：`db/schema.ts`、`players.ts`、新 migration |
| 2 | 武器分类与管理：`integrity/weapons.ts`、新 API/UI |
| 3 | 180 秒步兵窗口：`integrity/windows.ts`、`feed-events.ts` |
| 4 | 版本化可解释评分：`integrity/score.ts`、`integrity/decisions.ts` |
| 5 | Evidence 快照、置信度、留存：`integrity/evidence.ts`、新 migration |
| 6 | Dashboard/档案：`src/routes/(app)/server/[id]/integrity/`、玩家档案 |
| 7 | 举报服务、API；游戏聊天命令待确认可靠入口：`integrity/reports.ts` |
| 8 | 180–300 秒双语广播：现有 `broadcast` 规则及设置 UI |
| 9 | Discord 案件告警：`webhook-delivery.ts`、`webhooks.ts` |
| 10 | 24h/72h/7d 回放、默认 Dry Run：`integrity/replay.ts` |
| 11 | AUTO_KO：`integrity/decisions.ts`、`outbox.ts`、审计 |
| 12 | A/B 且当前行为异常的 AUTO_QUARANTINE、过期解除：`integrity/decisions.ts`、`poller.ts` |
| 13 | 同组织拒入和源标记同步：`lists.ts`、`lists-sync.ts` |
| 14 | 申诉和人工复核：`integrity/review.ts`、新 API/UI |
| 15 | 历史统计、回放校准和容量压测：`integrity/replay.ts`、分析 UI |

每个 Phase 独立提交、可单独回退。文档最终新增 `docs/integrity-system.md` 并在 `README.md` 声明：This is NOT a client anti-cheat. It does not inspect player devices. It is a server-side behavioral integrity and moderation system.

## 本机运行核查

审计时 `gh`、`bun`、`docker` 均不在 PATH，故尚未启动服务，也未运行项目测试。仓库已有 `docs/getting-started.md`、`.env.example`、`docker-compose.yml`：具备 Docker Compose 的环境中填充随机 `BETTER_AUTH_SECRET`、`ENCRYPTION_KEY`、`RELAY_SECRET`、`POSTGRES_PASSWORD` 与正确 `ORIGIN` 后运行 `docker compose up -d --build`；启动成功后可用内建 demo server 检查 UI/工作流，无须真实服务器。不要把真实密钥提交到仓库。
