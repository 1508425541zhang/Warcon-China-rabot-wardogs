# Warcon Rust 后端

全部业务 API、页面查询／表单操作及后台任务由 Rust 执行。Svelte 页面通过 Node.js 渲染及传输；原 TypeScript 后端仅保留为离线对照与类型参考。

当前工作保留在 `codex/rust-backend-refactor`，没有部署生产服务器。范围、验证和未进行的外部联调见 [迁移记录](../docs/rust-backend.zh-CN.md)，完整运行方法见 [开发指南](../docs/rust-development.zh-CN.md)。

## 二进制

| 名称            | 职责                                                          |
| --------------- | ------------------------------------------------------------- |
| `warcon-api`    | 身份、权限、全部 API、页面控制器、SSE                         |
| `warcon-worker` | 数据库租约、RCON 调度／relay、轮询、Feed 消费、风控与后台任务 |
| `warcon-model`  | expanded30m 或原有 27 通道长窗 CPU 推理                       |
| `migrate`       | 执行并核验原有 SQL 迁移                                       |
| `reset-auth`    | 管理员本地恢复账号，输出一次性临时密码；不是公开 API          |
| `warcon-health` | 本地服务健康检查，支持模型令牌                                |
| `model_fixture` | 离线对照真实 expanded30m 检查点                               |

## 构建与回归

需要 Rust 1.98.1、OpenSSL 开发库、PostgreSQL。Windows 使用配置完整的 MSVC 或 GNU 工具链。

```sh
cargo build --manifest-path backend/Cargo.toml --locked --bins
# 此数据库账号必须能创建测试数据库；测试自行清理 warcon_rust_test_*。
export TEST_DATABASE_URL='postgresql://postgres:password@127.0.0.1:5432/postgres'
cargo test --manifest-path backend/Cargo.toml --locked -- --include-ignored
```

数据库合同测试默认忽略，完整回归必须加 `--include-ignored`；不能把跳过数据库测试描述成整套验证通过。原实现的冻结对照文件在 `backend/fixtures`。生成命令见 `.github/workflows/rust-backend.yml`，不要手工修改期望值迎合 Rust。

所有 147 个原有 HTTP 操作和 51 个页面入口的迁移登记见 [接口清单](../docs/rust-route-inventory.json)。`backend/tools/update-inventory.ts` 更新登记；`page-contracts.d.ts` 是迁移前保存的 DTO，不能对已替换的适配器重新运行一次性快照工具。

## 原生模型

`warcon-model` 直接读取 Float32 权重、manifest、scaler、校准和冻结特征契约。expanded30m 通过只读事务读取已登记任务，原有 27 通道保持四表 JSON 输入。服务端核对玩家／服务器／对局范围，同一时刻只运行一个推理，不自行更新案件或处罚。

模型使用 `Authorization: Bearer <MODEL_API_TOKEN>`，包括 `GET /v1/health` 和 `POST /v1/assess`。请求上限 8 MiB。30 分钟输入采用 30 秒时间桶；缺失掩码、距离及武器信息保持原契约。

短窗树模型直接解释 `services/short-risk/artifacts-rust` 中的 JSON；Worker 每 10 秒读取独立窗口，不占用 Feed 消费队列。`WARCON_SHORT_RISK_ENABLED=0` 关闭执行，观察继续运行；设置为 `1` 才允许在现有保护通过后执行。

`export-short-model.py` 仅离线转换受信任的本地训练文件，不属于部署依赖。生产 API、Worker、长窗及短窗均不运行 Python 或 Bun。
