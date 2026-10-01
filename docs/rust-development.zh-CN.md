# Rust 后端：开发、回归与升级

本指南针对 `codex/rust-backend-refactor`。数据库、API、Worker、模型和页面渲染分开运行。以下开发步骤使用独立数据库，不修改已有云端配置。

## 1. 使用独立 Docker 开发编排

准备 Docker Engine／Docker Desktop、Compose 和 Git。进入仓库根目录，复制模板：

```powershell
Copy-Item .env.rust.dev.example .env.rust.dev
```

Linux/macOS 使用 `cp .env.rust.dev.example .env.rust.dev`。

填写 `.env.rust.dev`：

| 设置                        | 要求                                                  |
| --------------------------- | ----------------------------------------------------- |
| `ORIGIN`                    | `http://localhost:4302`；浏览器必须使用完全相同的地址 |
| `POSTGRES_PASSWORD`         | 独立开发数据库密码                                    |
| `BETTER_AUTH_SECRET`        | 至少 32 字符的随机会话签名密钥                        |
| `ENCRYPTION_KEY`            | 32 个随机字节的 Base64 编码                           |
| `RELAY_SECRET`              | 至少 32 字符，API 和 Worker 使用相同值                |
| `RUST_FRONTEND_TOKEN`       | 另一个至少 32 字符的随机值；Node 和 API 使用相同值    |
| `WARCON_SHORT_RISK_ENABLED` | 开发先保持 `0`，只观察短窗模型                        |

每个密码／密钥使用不同的随机值。Linux/macOS 可用 `openssl rand -base64 32`；PowerShell 可用以下命令，每次生成一个新值：

```powershell
$taskBytes = New-Object byte[] 32
$taskRng = [Security.Cryptography.RandomNumberGenerator]::Create()
$taskRng.GetBytes($taskBytes)
[Convert]::ToBase64String($taskBytes)
$taskRng.Dispose()
```

不要保留 `replace-with-...`，也不要提交环境文件。随后运行：

```sh
docker compose --env-file .env.rust.dev -f compose.rust-dev.yml config -q
docker compose --env-file .env.rust.dev -f compose.rust-dev.yml up -d --build
docker compose --env-file .env.rust.dev -f compose.rust-dev.yml ps -a
```

**必须同时传 `--env-file` 和 `-f`**：服务的 `env_file` 不会自动给 Compose 自身的 `${...}` 插值提供变量。

预期：`db`、`worker`、`api`、`warcon` 健康，`migrate` 成功退出。打开 `http://localhost:4302/setup` 创建首个账号；项目没有默认网页登录密码。开发使用专属 `warcon-rust-dev-db` 卷，页面只绑定本机 4302，数据库和内部服务端口不公开。

如果 Windows 报套接字访问权限错误，可用 `netsh interface ipv4 show excludedportrange protocol=tcp` 检查 Hyper-V／WSL 保留端口。选择空闲端口，例如在 `.env.rust.dev` 同时设置 `RUST_FRONTEND_PORT=14302` 和 `ORIGIN=http://localhost:14302`，再启动。不要只改其中一个；浏览器地址必须与 `ORIGIN` 一致。

没有游戏服务器时，可在组织中添加主机 `demo`、端口 `7776`、RCON 密码 `demo` 的演示服务器。这是独立于网页登录的模拟 RCON 凭据。模拟玩家、对局、配置和操作均由 Rust Worker 实现，内存状态随 Worker 重启重置。默认允许演示服务器，设置 `ALLOW_DEMO_SERVER=false` 可关闭。要查看模拟击杀和 KPM，先在该服务器配置页创建并启用 Kill Feed 令牌；Worker 会把模拟事件送入相同的原生消费链。演示事件仅属于明确添加的演示服务器，不会灌入真实服务器。

`MOCK_LIVE_BUILD=true` 可复现原版指定游戏构建缺少部分接口、启动参数锁定配置的表现；`MOCK_RATE_LIMIT_EVERY=N` 每 N 次模拟请求返回 429。这些仅供开发验证。

```sh
docker compose --env-file .env.rust.dev -f compose.rust-dev.yml logs --tail=80 api worker warcon
docker compose --env-file .env.rust.dev -f compose.rust-dev.yml down
```

`down` 保留开发数据。`down --volumes` 删除开发数据库，只有确实要重建时才使用。

## 2. 直接运行原生程序

需要 Rust 1.98.1、Node.js ≥22.12、Bun（前端依赖和离线测试工具）及 PostgreSQL。Linux 安装 `pkg-config`、OpenSSL 开发库；Windows 配置完整 MSVC 或 GNU 编译工具链。

```sh
bun install --frozen-lockfile
bun run prepare:backend
cargo build --manifest-path backend/Cargo.toml --locked --bins
```

Rust 二进制**不自动读取 `.env`**。Linux 可在当前终端加载只由自己编辑的配置：

```sh
set -a
. ./.env.rust.dev
set +a
export PGHOST=127.0.0.1 PGPORT=5432 PGUSER=warcon PGDATABASE=warcon_rust_dev
export PGPASSWORD="$POSTGRES_PASSWORD"
export ORIGIN=http://localhost:5173
export RUST_BACKEND_URL=http://127.0.0.1:4300
export RUST_BACKEND_BIND=127.0.0.1:4300
export RUST_WORKER_BIND=127.0.0.1:4301
export RELAY_URL=http://127.0.0.1:4301
```

提前创建空的 `warcon_rust_dev` 数据库。也可设置 `DATABASE_URL`，它优先于上述 `PG*`；URL 中密码的特殊字符必须百分号编码。使用 `PGPASSWORD` 可避免 URL 编码错误。

PowerShell 使用 `$env:NAME='value'` 设置同名变量；每个新终端都须设置相同的密钥。或只读导入自己创建的模板值：

```powershell
foreach ($taskLine in Get-Content -LiteralPath .env.rust.dev) {
    if ($taskLine -match '^([A-Z][A-Z0-9_]*)=(.*)$') {
        [Environment]::SetEnvironmentVariable($Matches[1], $Matches[2], 'Process')
    }
}
$env:PGHOST='127.0.0.1'
$env:PGPORT='5432'
$env:PGUSER='warcon'
$env:PGDATABASE='warcon_rust_dev'
$env:PGPASSWORD=$env:POSTGRES_PASSWORD
$env:ORIGIN='http://localhost:5173'
$env:RUST_BACKEND_URL='http://127.0.0.1:4300'
$env:RUST_BACKEND_BIND='127.0.0.1:4300'
$env:RUST_WORKER_BIND='127.0.0.1:4301'
$env:RELAY_URL='http://127.0.0.1:4301'
```

按顺序运行：

```sh
cargo run --manifest-path backend/Cargo.toml --bin migrate -- drizzle
# 新终端：Worker 自身的 RELAY_URL 必须为空，防止中继给自己。
RELAY_URL='' cargo run --manifest-path backend/Cargo.toml --bin warcon-worker
# 另一个终端：API 保持上面的 RELAY_URL。
cargo run --manifest-path backend/Cargo.toml --bin warcon-api
# 另一个终端：只运行页面渲染和传输。
bun run dev -- --host localhost --port 5173
```

PowerShell 在启动 Worker 的那个终端先执行 `$env:RELAY_URL=''`，其余终端保留原值。访问 `http://localhost:5173/setup`。生产页面构建用 `node node_modules/vite/bin/vite.js build`，然后 `node build/index.js`。

`RUST_FRONTEND_TOKEN` 用 HMAC 证明真实客户端地址；Rust 不信任用户传入的裸转发头。没有证明时只看到直接对端地址，不能依靠任意 `X-Forwarded-For` 绕过限流。反向代理部署还需让 Node 读取由可信代理设置的地址头。

开发 Svelte 代码插件时，在 `src/lib/plugins/catalog.ts` 登记 manifest，并在 `components.ts` 登记组件。`bun run prepare:backend` 为 Rust 导出同一份经过 schema 校验的目录；直接重新编译 Rust 前先运行它。两个 Docker 构建均会自动生成目录，避免前端有组件而后端仍拒绝其 renderer。

## 3. 模型配置

### 短窗

直接运行 Worker 时设置：

```sh
export SHORT_RISK_MODEL_PATH='/absolute/path/to/services/short-risk/artifacts-rust/isolation.json'
export WARCON_SHORT_RISK_ENABLED=0
```

Compose 已指定原生 JSON 模型路径。`0` 保留观察、关闭执行；`1` 允许在原有连续窗口和保护条件通过后执行。风控页面显示原生快照。XGBoost 也可使用同目录对应模型文件；必须保留其 manifest、校准及原模型身份。

### 长窗

把**完整训练产物**放入 `models/long`，包含原来的 manifest、权重、scaler、校准、武器与距离基线。不要只复制检查点或自己拼一个 manifest；启动时会核对文件哈希及维度。该目录已从 Git 排除。

填写 `MODEL_ARTIFACTS_PATH` 和独立随机 `MODEL_API_TOKEN`，然后：

```sh
docker compose --env-file .env.rust.dev -f compose.rust-dev.yml --profile model up -d --build
```

在组织风控模型设置中填写内部地址 `http://model:8091` 及相同令牌，测试后开启模型模式。只启动模型容器不会自动改变组织规则或启用处罚。

直接运行：

```sh
export MODEL_ARTIFACTS_DIR='/absolute/path/to/full/artifacts'
export MODEL_DATABASE_URL='postgresql://readonly-user:password@127.0.0.1:5432/warcon_rust_dev'
export MODEL_API_TOKEN='your-independent-random-token-at-least-32-characters'
export MODEL_HOST=127.0.0.1 MODEL_PORT=8091
cargo run --manifest-path backend/Cargo.toml --release --bin warcon-model
```

expanded30m 只读已登记任务的数据，建议使用只读数据库账号。原有 27 通道保持四表 JSON 契约。模型的 `/v1/health` 也要求 Bearer；Worker relay 的 `/health` 是内部健康检查。

## 4. 完整回归

```sh
export TEST_DATABASE_URL='postgresql://postgres:password@127.0.0.1:5432/postgres'
cargo test --manifest-path backend/Cargo.toml --locked -- --include-ignored
bun run test
bun run check
node node_modules/vite/bin/vite.js build
```

测试数据库账号必须有 `CREATE DATABASE` 权限。Rust 的每个数据库测试使用独立 `warcon_rust_test_*`，结束后删除；不使用生产数据库。`bun run test` 保留前端／原业务纯函数对照；旧 TypeScript HTTP 集成测试不再是运行后端，原生 HTTP 合同由 Rust 测试覆盖。

`backend/tests/pages_contract.rs` 检查全部页面、必需返回字段和深层权限；`upgrade_contract.rs` 验证单消费者旧任务升级。生成冻结对照文件的完整命令见 `.github/workflows/rust-backend.yml`。

构建二进制与页面后，可以运行真实本地多进程联调：

```sh
SMOKE_PSQL=psql \
SMOKE_DATABASE_URL='postgresql://postgres:password@127.0.0.1:5432/postgres' \
SMOKE_BIN_DIR=backend/target/debug \
SMOKE_MODEL_ARTIFACTS=/absolute/path/to/model/artifacts \
node backend/tools/local-stack-smoke.mjs
```

它只在本机 4310—4313 启动临时服务，创建并清理独立数据库，使用生成的测试凭据，输出联调结果。Windows 用 `$env:SMOKE_...` 设置同名变量和本机 `psql.exe` 路径。结果文件 `backend/local-stack-smoke.json` 已忽略。

真实 expanded30m 数值对照：

```sh
cargo run --manifest-path backend/Cargo.toml --release --bin model_fixture -- /path/to/artifacts
```

必须包含该检查点对应的 `parity.json`。真实权重对照、合成 HTTP 协议测试和外部服务联调是不同验证，不能互相替代。

## 5. 旧部署升级演练

本任务不执行云端部署。未来先在独立环境用备份演练：

1. 保存数据库备份、原 `.env` 和模型产物，尤其保留原会话／加密密钥。
2. 恢复到独立数据库；先核对原 SQL 迁移历史，再运行 `migrate drizzle`。
3. 停止这个测试数据库对应的旧 Worker，等租约释放；再启动 Rust Worker。不要同时运行两套业务 Worker。
4. API 和 Node 使用同一个 `ORIGIN`、相同 `RUST_FRONTEND_TOKEN`；API 与 Worker 使用相同 `RELAY_SECRET`。
5. 验证旧账号密码／OTP／Passkey、组织权限、旧案件、名单、双消费者、模型文件身份和新的事件流。
6. 对模拟游戏服务验证处罚和广播；实际服务器联调另选明确的测试服。
7. 需要回滚时使用匹配版本的数据库备份和旧镜像；不要假设删除新增迁移记录就能回滚。

主 `docker-compose.yml` 保留原数据库卷和数据库镜像版本，增加原生 API，并拆开 Node 和 Rust 镜像。独立开发请始终使用 `compose.rust-dev.yml`。

忘记开发账号密码时：

```sh
docker compose --env-file .env.rust.dev -f compose.rust-dev.yml run --rm worker reset-auth your-username
```

这会清除该账号的 OTP、Passkey、会话和恢复密钥，保留 Steam／Discord 关联，并输出临时密码，要求下次修改。只有确实需要恢复时才执行。

## 6. 当前验证边界

已完成本地原生服务、数据库迁移、模型数值及 Node 页面联调；两个 Docker 镜像也已实际构建，独立容器环境的 10 项端到端检查通过，包含真实长窗模型、演示玩家和击杀双消费链。验证记录见 [迁移与验证记录](rust-backend.zh-CN.md)。CI 使用相同检查，尚未推送运行；Steam／Discord、QQ、AI 的真实账号连接需在独立环境补充验证。

`backend/tools/local-stack-smoke.mjs` 可用 `SMOKE_BASE_PORT` 指定连续四个联调端口，默认 4310–4313；例如 Windows 下设置 `$env:SMOKE_BASE_PORT='18100'` 再运行。`container-stack-smoke.mjs` 仅用于新建的 `warcon-rust-validation` 空数据库，会创建随机临时账号，不用于已有数据。不要针对生产 Compose 运行这类初始化测试。
