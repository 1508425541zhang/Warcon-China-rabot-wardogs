# Warcon Rust 后端开发分支

最终范围：所有业务 API、页面服务端业务逻辑和后台任务迁到 Rust，保留 Svelte 页面。

**当前是迁移中的开发代码，尚未替换完整后端，也没有接入现有页面或部署云端。**
完整清单在 [`../docs/rust-route-inventory.json`](../docs/rust-route-inventory.json)，迁移说明在
[`../docs/rust-backend.zh-CN.md`](../docs/rust-backend.zh-CN.md)。

## 本地运行

准备 Rust 1.98.1、PostgreSQL，以及独立的开发数据库。Windows 的 MSVC 工具链需要
Visual Studio C++ Build Tools；GNU 工具链需要完整、兼容的 MinGW binutils。
Linux 构建需要 `pkg-config` 和 OpenSSL 开发库。运行服务使用本地环境变量，程序不会自动读取 `.env`。

```sh
cd backend
export DATABASE_URL='postgresql://postgres:your-password@127.0.0.1:5432/warcon_rust_dev'
export ORIGIN='http://localhost:3000'
export BETTER_AUTH_SECRET='your-development-secret-at-least-32-characters'
export ENCRYPTION_KEY='your-development-base64-key'
cargo run --bin migrate -- ../drizzle
cargo run --bin warcon-api
```

PowerShell 用 `$env:DATABASE_URL = '...'` 设置变量，其余命令相同。
默认监听 `127.0.0.1:4300`。API 可使用现有格式的签名登录 Cookie 或开发数据库里的 API Key。
没有实现的路由返回 404，不会暗中转回 Bun。

## 验证

```sh
cargo test --locked
# 该账号须能 CREATE DATABASE；测试会创建并删除自己的 warcon_rust_test_* 数据库。
export TEST_DATABASE_URL='postgresql://postgres:your-password@127.0.0.1:5432/postgres'
cargo test --locked --test database_contract -- --ignored
# 模型文件仍保存在本地训练目录，传入包含 manifest.json / parity.json 的目录。
cargo run --release --bin model_fixture -- /path/to/model/artifacts
```

委员会和 Feed 对照文件从旧 TypeScript 实现生成，不能为了让 Rust 测试通过而手动改期望值：

```sh
cd ..
bun backend/fixtures/generate-committee.ts
bun --tsconfig-override backend/fixtures/tsconfig.json backend/fixtures/generate-feed.ts
bun backend/fixtures/generate-analytics.ts
bun backend/fixtures/generate-settings.ts
bun backend/fixtures/generate-model-features.ts
bun backend/fixtures/generate-matches.ts
cargo test --manifest-path backend/Cargo.toml --locked
```

## 已迁移的账号与管理接口

Rust 已实现密码登录、OTP／备用码、Steam／Discord OAuth、Passkey 和恢复密钥。
原有 scrypt 密码、OTP 密文与签名 Cookie 可继续读取，不要求重新设置所有账号。
旧 Passkey 首次验证成功后绑定原有 userHandle，新增的迁移 `0073` 只增加可空列。

用户、组织、成员、邀请、服务器、权限、名单、个人插件、公开状态／排行榜／生涯与对局接口
已有原生实现；RCON 的 40 个动作按原版请求格式执行。名单只撤销面板管理的预留位，
游戏服自有条目保留，组织封禁通过面板踢出执行，已接入原生轮询。

前端尚未切换，这些接口目前由独立开发数据库和本地模拟游戏服验证。
`RUST_FRONTEND_TOKEN` 是将来前端转发客户端地址所用的签名密钥，至少 32 字节；
Rust 不信任裸 `X-Forwarded-For`。

## Worker 当前接入情况

`warcon-worker` 当前注册轮询、玩家会话、对局、原始观察归档、名单同步、Steam 档案、游戏时长、小时汇总、Integrity 击杀消费、基线重建、长窗队列、处罚发送、Webhook 和 QQ 任务，**还不能替代完整生产 Worker**。
它复用现有 `worker_ownership` 租约；其他 Worker 拥有数据库时会拒绝启动。
Integrity 队列先核对持久化事件身份和阵营前后快照，再进行评估与建案，业务成功后才确认任务。
Legacy 消费者的自动化动作仍待迁移，它的待处理任务保持原状。

```sh
export STEAM_API_KEY='your-development-key'
export RUST_WORKER_BIND='127.0.0.1:4301'
export RELAY_SECRET='your-development-relay-secret-at-least-32-characters'
cargo run --bin warcon-worker
```

API 进程设置 `RELAY_URL=http://127.0.0.1:4301` 和相同 `RELAY_SECRET`，让游戏请求进入
Worker 的唯一调度队列。动作不会因网络失败自动重发。排队后重新验证账号、权限和 Worker 租约。
`GET /api/live` 读取缓存；`GET /api/live/events?servers=<id>` 提供命名 SSE 事件，
每五秒重新核对访问权限，掉线或撤权后关闭订阅。API 与 Worker 通过 PostgreSQL 通知通信。

轮询测试覆盖原始日志、重连、计数重置、换图、限流、下线、租约失效和实时权限。
自动化触发器、AI 审核、证据导入／保留、页面 load/actions 和前端切换仍在迁移清单中。

## 风控、通知与 QQ

Rust 已接入旧评分、五专家投票、逐案件地图／人数基线、个人历史、不可变案件证据和人工审核。
人工确认违规的七天封禁与审核记录一起提交；自动处置保留人数、数据健康、VIP、冷却和频率保护。
发送前持久化状态，超时或进程中断时标记结果未知，不重复发送。面板隔离已生效时，游戏踢出失败不会撤销隔离。

基线以分页历史重放计算，每个案件只排除自身相关事件，样本按玩家和日期限制权重。
新建案件会原子写入通知输入；Discord 排队发送、状态卡片、429 延后、撤权清理均由 Rust 完成。

QQ 支持官方机器人及 OneBot 接口：签名接收、官方心跳／恢复、绑定和旧版迁移、暖服积分、地图投票、
友方逐人广播、预留位兑换、举报和管理员订单结算。重复指令不重复扣分，结果未知的订单只允许人工核实。
密钥沿用现有加密格式。`0074` 保留小数风险分，`0075` 增加通知输入和投递队列，不修改原始击杀数据。

对照和数据库验证：

```sh
bun backend/fixtures/generate-integrity.ts
bun backend/fixtures/generate-qq.ts
cargo test --manifest-path backend/Cargo.toml --test integrity_parity --test qq_parity
cargo test --manifest-path backend/Cargo.toml --test integrity_pipeline_contract --test integrity_consumer_contract --test webhook_contract --test qq_contract -- --include-ignored
```

## 原生长窗模型服务

`warcon-model` 支持现有 expanded30m 和原有 27 通道模型的 Float32 权重与冻结特征契约，
不运行 Python 或 Bun。模型身份、文件哈希、30 分钟序列、30 秒时间桶和缺失掩码均保留。
expanded30m 只读取数据库中已经登记的 `integrity_model_runs`；27 通道模型读取原版四表 JSON。
推理服务不会自行写入处罚或完成任务。

```sh
export MODEL_ARTIFACTS_DIR='/path/to/expanded30m/artifacts'
export MODEL_DATABASE_URL='postgresql://readonly-user:password@127.0.0.1:5432/warcon_rust_dev'
export MODEL_API_TOKEN='your-random-token-at-least-32-characters'
export MODEL_HOST='127.0.0.1'
export MODEL_PORT='8091'
cargo run --release --bin warcon-model
```

请求使用 `Authorization: Bearer <MODEL_API_TOKEN>`：

- `GET /v1/health`：返回原有完整模型 manifest。
- `POST /v1/assess`：输入包含 `schema`、已登记的 `requestId` 和 `sources.matches[0].id`。
  服务核对任务所属服务器、玩家和对局，返回 `READY` 或 `INSUFFICIENT_DATA`。
- 同时只处理一个推理，繁忙时返回 503；请求上限 8 MiB；数据库查询超时 10 秒。

数据读取采用只读事务；建议另外限制数据库账号为 SELECT 权限。
27 通道模型仍要求单个玩家、单局和带时区的时间戳，保留原有 60／200 个时间桶的检查。
两份真实检查点的六组评分、十五组四表特征和五组完整响应已与原版 Bun 对照。

## 原生短窗模型

短窗 IsolationForest 与 XGBoost 由 Rust 直接解释 JSON 树文件，无需 Python 或 Bun 运行服务。
`services/short-risk/artifacts-rust` 包含现有训练结果的原生导出、哈希及原检查点身份。
24 项特征、60／120 秒窗口、双时钟过滤和缺失值处理保持原版含义。

```sh
export SHORT_RISK_MODEL_PATH='/path/to/services/short-risk/artifacts-rust/isolation.json'
export WARCON_SHORT_RISK_ENABLED='1'
cargo run --bin warcon-worker
```

Worker 每十秒独立读取当前对局数据，不领取 Feed 或长窗任务。
P99.6 为警惕；实际踢出沿用现有连续五窗 P99.9、1:2:3:4:5 加权及人数、数据健康、VIP、频率保护。
关闭执行开关时继续推理。状态写在开发数据库的 `rust:shortRiskSnapshot`，前端适配仍待完成。
请求结果不确定时保存 `unknown`，同一玩家／场次的已尝试动作不会自动重发。

`backend/tools/export-short-model.py` 只用于离线转换可信的本地训练文件；不属于部署依赖。
它先核对原文件哈希，再生成树文件与数值对照；禁止对不可信 pickle/joblib 使用该转换工具。
测试覆盖两种真实模型的 256 组评分、36 组特征，以及原版窗口、断流和自动处置行为。

测试凭证均为合成值。不要把线上密码或密钥写进 Git。
