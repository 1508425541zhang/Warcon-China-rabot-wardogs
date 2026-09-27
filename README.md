<p align="center">
  <img src="branding/wacom-hero.svg" alt="Warcon CN：WARDOGS 中文服务器管理与社区风控，从游戏事件到可复核证据" width="100%">
</p>

# Warcon CN

**面向 WARDOGS 社区服务器的开源中文管理面板。** 在 [Warcon](https://github.com/warcon-app/warcon) 的多服务器 RCON 管理基础上，加入 Kill Feed 事件分析、玩家档案、五专家风控、证据案件、管理员审核和可配置自动化。支持 Docker Compose 自托管。

*A self-hosted Chinese WARDOGS server management panel with RCON, Kill Feed, evidence review and server-side anti-cheat signals.*

[快速开始](#快速开始) · [功能总览](#功能总览) · [风控如何工作](#风控如何工作) · [安装指南](docs/install.zh-CN.md) · [功能文档](docs/features.zh-CN.md) · [技术架构](docs/architecture.zh-CN.md) · [许可证](LICENSE)

> **运行边界：** 新安装默认处于统计影子模式，自动处罚开关关闭。委员会可形成观察或待审核案件；只有明确开启自动踢出且满足证据门槛与保护条件，才会执行踢出。AI 辅审提供建议，不自行封禁。项目不扫描玩家电脑，也不提供自动永久封禁。

## 适合谁使用

- **社区服管理员：** 在一个中文界面查看多台 WARDOGS 服务器、在线玩家、地图、封禁、审计和自动化设置。
- **风控审核员：** 从击杀事件、KPM、专家票与历史资料进入证据案件，查看依据并记录审核结论。
- **自托管部署者：** 用 Docker Compose 运行网页、Worker 和 PostgreSQL/TimescaleDB；先体验模拟服务器，再接入真实游戏服。

Warcon CN 是**服务器侧管理与行为分析工具**。它依赖游戏服务器实际提供的 RCON 状态和 Kill Feed；没有收到事件时，不会把未知 KPM 当作 0，也不会凭空得出作弊结论。

## 快速开始

准备 [Docker Desktop / Docker Engine 与 Compose](https://docs.docker.com/get-started/get-docker/) 和 [Git](https://git-scm.com/downloads)。Compose 会构建项目并启动数据库，无需另外安装 Bun 或 PostgreSQL。

```bash
git clone https://github.com/1508425541zhang/warcon-cn.git
cd warcon-cn
```

复制配置模板：Windows PowerShell 使用 `Copy-Item .env.example .env`；macOS/Linux 使用 `cp .env.example .env`。编辑 `.env`，为 `BETTER_AUTH_SECRET`、`ENCRYPTION_KEY`、`RELAY_SECRET`、`POSTGRES_PASSWORD` 设置**四个不同的随机值**，本机试用保留 `ORIGIN=http://localhost:3000`。生成随机值的逐平台命令见[安装指南第 2 步](docs/install.zh-CN.md#第-2-步复制配置模板)。不要把 `.env` 提交到 GitHub。

```bash
docker compose config -q
docker compose up -d --build
docker compose ps -a
```

`db` 显示 healthy、`warcon` 与 `worker` 显示 running、`migrate` 显示 exited (0)，即为预期状态。在本机打开 **[http://localhost:3000/setup](http://localhost:3000/setup)**，自行创建第一个所有者账号。**项目没有默认网页登录账号或密码**；`POSTGRES_PASSWORD` 只是数据库密码。

还没有游戏服务器？登录后创建组织，在“服务器 → 添加服务器”中填写主机 `demo`、端口 `1`、协议 `http`、RCON 密码 `demo`，即可预览管理界面。模拟服务器用于体验页面与流程，不代表真实玩家事件或足够的统计样本。

公网部署、更新、排障或接入真实 WARDOGS 服务器，请按[从零安装指南](docs/install.zh-CN.md)逐步操作。真实服务器的 **RCON** 和 **Kill Feed** 必须分别配置：前者是面板连向游戏服，后者是游戏服向面板回传击杀。仅 RCON 连通，不会自动产生 KPM。

## 功能总览

| 领域 | 当前提供的能力 | 进一步阅读 |
| --- | --- | --- |
| 服务器管理 | 多服务器 RCON、组织与角色权限、玩家操作、地图轮换、封禁／预留位、审计、公开状态页 | [中文使用说明](README.zh-CN.md) |
| 玩家数据 | SteamID64 档案、击杀／死亡／KD、180 秒纯步兵 KPM、武器击杀距离参考图、相邻对局留存及 Steam 游戏时长分布 | [功能与数据边界](docs/features.zh-CN.md) |
| 社区风控 | Legacy 可调评分、统计影子模式、五专家委员会、版本化规则、历史基线和可解释的未知状态 | [委员会 v3](docs/committee-v3.zh-CN.md) |
| 证据与审核 | 案件分页归档、事件与规则快照、审核结论、人工确认违规后的本服七天封禁 | [案件审核](docs/case-review-penalty.zh-CN.md) |
| 自动化 | 禁止换边、武器限制、强弱阵营平衡、KPM／KD／金钱效率上限、疑似组队名单和赛后荣誉广播；各项独立配置 | [功能介绍](docs/features.zh-CN.md#自动化页面) |
| 可选扩展 | 兼容 OpenAI 接口的 AI 案件辅审、Steam 公开资料、Discord 通知、JSON／JSONL 历史导入、个人插件 | [AI 辅审](docs/integrity-ai.zh-CN.md) · [插件开发](docs/personal-plugins.zh-CN.md) |

主界面以简体中文呈现；游戏返回的地图、武器、协议字段和部分原始错误可能保留原文。没有可信来源的指标会标为不可用或已确认下限，不能靠界面设置补出未采集的事件。

## 风控如何工作

<p align="center">
  <img src="branding/wacom-flow.svg" alt="Warcon CN 数据流程：Kill Feed、有效性检查、行为信号、专家投票、证据案件与管理员复核" width="100%">
</p>

1. **接收与校验：** 游戏服将 Kill Feed 回传给面板。事件经过令牌、去重、玩家身份、武器分类、阵营及时间检查后，可靠的纯步兵击杀才进入对应 KPM 计算。
2. **形成可解释信号：** 展示滚动 180 秒纯步兵 KPM、独立受害者、短时爆发等。五专家分别分析击杀节奏、精准度、生涯偏差、本局变化点和持续异常。缺少样本的专家投“未知”，不算正常票，也不算异常票。
3. **建案与复核：** 同一次评估中，至少 3 票可疑或至少 2 票极可能作弊，生成待审核案件。案件保存证据、规则版本及审核和执行记录；AI 可给出理由及数字核对建议，但不替管理员确认违规。
4. **受保护的动作：** 至少 3 票极可能作弊，或 **180 秒步兵 KPM 严格大于 4 且击杀节奏之外另有一位专家至少可疑**，可进入自动踢出通道。实际执行还须组织显式开启，并通过人数、数据健康、参考样本、频率等保护。管理员人工确认违规可执行本服七天封禁。

这些是当前[五专家委员会 v3](docs/committee-v3.zh-CN.md)的概要。Legacy 是独立的可调分数路径；其 KPM 分段、KD 展示和动作开关见[技术架构](docs/architecture.zh-CN.md)。**看到高分或案件，不等于玩家已经被踢出或封禁。** 距离分布图等参考图不直接作为处罚条件。

## 数据来源与限制

| 数据来源 | 可用于什么 | 必须了解的限制 |
| --- | --- | --- |
| WARDOGS RCON | 在线玩家、地图、管理命令和状态观察 | RCON 连通不等于 Kill Feed 已回传 |
| Kill Feed | 击杀事件、KPM、武器与部分行为证据 | 未上报的伤害、断线期间事件不能补造；步兵分类需要可靠阵营和武器信息 |
| Steam Web API（可选） | 已公开的账号、封禁和 WARDOGS 累计游戏时长 | 不是官方完整战斗生涯；资料不公开时显示未知 |
| 管理员及社区记录 | 人工审核、网页举报、经审核的外服历史导入 | 外服历史保留来源，不冒充本服实时事件 |
| AI 提供商（可选） | 对已保存案件 JSON 做理由说明与数字核对 | 需要管理员配置兼容接口；建议不直接封禁 |

目前没有已验证的**游戏内聊天读取**接口，因此不宣称 `!report`／`!BAN` 已可用。项目不读取玩家 IP，不根据延迟推测地区，不扫描客户端进程。工程／医疗贡献缺少可靠来源时，不生成相应奖项。详见[完整功能说明](docs/features.zh-CN.md)。

## 部署架构

默认 Compose 包含 `migrate`、`warcon`、`worker`、`db`：迁移先执行；SvelteKit 网页负责身份、权限和 API；Worker 轮询游戏服并处理后台任务；PostgreSQL/TimescaleDB 保存观察、规则和案件。浏览器不直接连接游戏 RCON，也不持有 RCON 密码。公网安装需要正确的 `ORIGIN`、可信 HTTPS 以及游戏服可访问的 Kill Feed 地址。

**技术栈：** Bun · SvelteKit · TypeScript · PostgreSQL/TimescaleDB · Docker Compose。代码结构、权限与事件流见[技术架构文档](docs/architecture.zh-CN.md)。

## 文档导航

| 你想做什么 | 从这里开始 |
| --- | --- |
| 第一次安装、设置账号、接入真实游戏服 | [中文安装与故障排查](docs/install.zh-CN.md) |
| 快速找到管理界面入口 | [中文使用说明](README.zh-CN.md) |
| 核对已实现功能和未接入数据 | [功能介绍](docs/features.zh-CN.md) |
| 配置委员会、审核案件与 AI | [委员会规则](docs/committee-v3.zh-CN.md) · [案件审核](docs/case-review-penalty.zh-CN.md) · [AI 辅审](docs/integrity-ai.zh-CN.md) |
| 导入其他服务器的历史事件 | [JSON／JSONL 导入规范](docs/integrity-import.zh-CN.md) |
| 扩展个人插件或调用插件 API | [插件开发手册](docs/personal-plugins.zh-CN.md) |
| 理解架构或对照原版 Warcon | [技术架构](docs/architecture.zh-CN.md) · [上游说明](README.upstream.md) |

## 常见问题

**安装后账号和密码是什么？** 没有预置账号。打开 `/setup` 创建第一个所有者账号；数据库密码不能登录网页。

**玩家在线，KPM 为什么显示“—”？** 先检查 Kill Feed 是否收到真实击杀，再看玩家阵营、武器分类和事件有效性。换地图或反复刷新页面不会补齐缺失数据。[安装指南第 6 步](docs/install.zh-CN.md#第-6-步有游戏服后再接入)有逐项检查方法。

**能先看界面再接游戏服吗？** 可以使用 `demo` 模拟服务器；统计基线和实际处罚仍需要真实可靠的事件与权限配置。

**AI 或委员会会自动封禁吗？** AI 不封禁。委员会自动动作需要明确开启并经过执行保护，且当前只提供踢出通道；人工确认违规的七天封禁是另一条流程。

## 来源、许可与反馈

项目基于开源 [Warcon](https://github.com/warcon-app/warcon)；本仓库遵循 [AGPL-3.0 许可证](LICENSE)。本项目由社区维护，不代表 WARDOGS、BULKHEAD 或 Team17 官方。遇到可复现的问题或希望改进文档，请到 [GitHub Issues](https://github.com/1508425541zhang/warcon-cn/issues) 描述版本、操作步骤和实际结果；提交日志前请移除密钥与玩家隐私信息。
