# Warcon 中文说明

Warcon 是面向 WARDOGS 社区服务器的自托管 RCON 面板。它使用 Bun、SvelteKit 和 PostgreSQL/TimescaleDB，Web 界面与 Worker 共用数据库，Worker 负责与游戏服务器通信。项目直接沿用上游 Warcon 的 WARDOGS RCON 客户端和已验证的 Kill Feed，不重新逆向协议。

> 本项目不是客户端反作弊软件，不扫描玩家设备。正在开发的社区完整性系统只使用服务器行为数据和经过授权的公开 Steam 信息；当前自动踢人、自动隔离均未启用。

## 当前进度

- SteamID64 组织档案、历史昵称和首次/最近出现时间。
- 可由组织所有者调整的武器分类；未知武器不计入纯步兵 KPM。
- 滚动 180 秒纯步兵 KPM、独立异常窗口、可解释风险分及证据案件，均处于仅记录阶段。
- “完整性 / Integrity”管理页支持简体中文与 English 切换。

完整设计与未完成项目见[实施计划](docs/wardogs-community-integrity-plan.zh-CN.md)和[功能状态文档](docs/integrity-system.md)。

## 使用 Docker Compose 运行

1. 安装 Docker 和 Docker Compose。
2. 复制 `.env.example` 为 `.env`，为 `BETTER_AUTH_SECRET`、`ENCRYPTION_KEY`、`RELAY_SECRET` 设置不同的长随机值，填写 `POSTGRES_PASSWORD` 和实际访问地址 `ORIGIN`。
3. 在仓库目录运行 `docker compose up -d --build`。
4. 打开 `ORIGIN`，完成所有者初始化。可以先用内置 demo server 查看面板，不必连接真实服务器。

详细部署、代理、备份和服务器接入说明见[上游英文 README](README.md)及[入门文档](docs/getting-started.md)。不要提交 `.env`。

## 重要限制

目前已确认的游戏接口能发送私聊和推送击杀事件，没有已验证的玩家聊天读取入口。因此游戏内 `!report` 和 `!BAN` 暂不可用，也不会把 `!BAN` 当成封禁命令。任何自动长期隔离都必须等证据可靠性、复核、申诉和故障降级完成后才允许开启。
