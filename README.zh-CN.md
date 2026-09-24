# Warcon China 中文说明

Warcon China 是基于 Warcon 的 WARDOGS 社区服务器自托管 RCON 面板与行为风控扩展。它使用 Bun、SvelteKit 和 PostgreSQL/TimescaleDB，Web 界面与 Worker 共用数据库，Worker 负责与游戏服务器通信。项目直接沿用上游 Warcon 的 WARDOGS RCON 客户端和已验证的 Kill Feed，不重新逆向协议。[项目首页](README.md)提供功能概览与插图，[技术文档](docs/architecture.zh-CN.md)说明系统边界和数据流。

> 本项目不是客户端反作弊软件，不扫描玩家设备。正在开发的社区完整性系统只使用服务器行为数据和经过授权的公开 Steam 信息；当前自动踢人、自动隔离均未启用。

## 当前进度

- SteamID64 组织档案、历史昵称和首次/最近出现时间。
- 可由组织所有者调整的武器分类；未知武器不计入纯步兵 KPM。
- 滚动 180 秒纯步兵 KPM、独立异常窗口、可解释风险分及证据案件，均处于仅记录阶段。
- “社区风控”管理页默认使用简体中文。组织所有者可在服务器的“社区风控 → 风控设置”调整附件中的 KPM 分段、风险阈值、辅助信号参数和武器分类；风险区间随输入实时预览，保存前检查阈值顺序，保存后生成规则版本并写入审计日志。
- 在线风控表和玩家档案显示击杀、死亡、KD、180 秒纯步兵 KPM、近 10 分钟峰值、独立受害者、风险等级和评分分项。KD 只供查看，不单独加分。
- 首次设置、登录、注册、管理后台、组织与服务器管理、自动化规则、统计页面和公开页面的主要界面文字已改为简体中文；RCON、SteamID64、配置键等需原样输入的标识保留。
- 已公开状态页的服务器提供中文举报表单；需登录并绑定 Steam。举报保存前后各 180 秒的相关击杀证据，按独立举报人计分。
- 管理页显示 24 小时、72 小时和 7 天的模拟运行影响预览；可选 Discord 案件提醒不会泄露举报人身份。

完整设计与未完成项目见[实施计划](docs/wardogs-community-integrity-plan.zh-CN.md)和[功能状态文档](docs/integrity-system.md)。

## 使用 Docker Compose 运行

1. 安装 Docker 和 Docker Compose。
2. 复制 `.env.example` 为 `.env`，为 `BETTER_AUTH_SECRET`、`ENCRYPTION_KEY`、`RELAY_SECRET` 设置不同的长随机值，填写 `POSTGRES_PASSWORD` 和实际访问地址 `ORIGIN`。
3. 在仓库目录运行 `docker compose up -d --build`。
4. 打开 `ORIGIN`，完成所有者初始化。可以先创建组织，然后添加主机 `demo`、端口 `1`、密码 `demo` 的内置模拟服务器查看面板，不必连接真实服务器。

详细部署、代理、备份和服务器接入说明见[上游英文 README](README.upstream.md)及[入门文档](docs/getting-started.md)。不要提交 `.env`。

## 重要限制

目前已确认的游戏接口能发送私聊和推送击杀事件，没有已验证的玩家聊天读取入口。因此游戏内 `!report` 和 `!BAN` 暂不可用，也不会把 `!BAN` 当成封禁命令。任何自动长期隔离都必须等证据可靠性、复核、申诉和故障降级完成后才允许开启。

游戏服务器、Steam 和 Discord 返回的原始名称、原因及少数后端错误消息可能仍为英文。爆头率、穿透率、短时击杀、Steam 处罚和公开游戏时间的规则参数可以预设，但目前没有可信数据源接入实时评分，界面已标明这一点。部署前需要 PostgreSQL/TimescaleDB。
