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
游戏服自有条目保留，组织封禁通过面板踢出执行。后台轮询尚未接入这条执行链。

前端尚未切换，这些接口目前由独立开发数据库和本地模拟游戏服验证。
`RUST_FRONTEND_TOKEN` 是将来前端转发客户端地址所用的签名密钥，至少 32 字节；
Rust 不信任裸 `X-Forwarded-For`。

## Worker 当前接入情况

`warcon-worker` 当前注册 Steam 档案、Steam 游戏时长和小时汇总任务，**还不能替代完整生产 Worker**。
它复用现有 `worker_ownership` 租约；其他 Worker 拥有数据库时会拒绝启动。
Feed 的两个队列已有领取、排序、重试和确认基础，但完整评估和处罚消费者尚未接入，
不会因为领到任务就把它标为处理完成。

```sh
export STEAM_API_KEY='your-development-key'
cargo run --bin warcon-worker
```

## 原生长窗模型服务

`warcon-model` 使用现有 expanded30m 模型的 Float32 权重和冻结的特征契约，
不运行 Python 或 Bun。模型身份、文件哈希、30 分钟序列、30 秒时间桶和缺失掩码均保留。
它只读取数据库中已经登记的 `integrity_model_runs`，不会自行写入处罚或完成任务。

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
这里只支持已迁移的 expanded30m 契约，原有 27 通道模型及短窗模型尚待迁移。

测试凭证均为合成值。不要把线上密码或密钥写进 Git。
