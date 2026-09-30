# NapCatQQ / LLBot / OneBot 11、暖服积分与兑换

WARCON 支持 NapCatQQ 和 LLBot（LuckyLilliaBot），通过 OneBot 11 的 HTTP 上报接收群指令，通过 HTTP API 回复。无需官方 QQ 开放平台 AppID 或群 openid。业务逻辑、积分账本和游戏 RCON 仍在 WARCON 内，机器人框架独立运行并登录机器人 QQ。

## 网页配置（推荐）

以站点所有者身份打开 **站点管理 → QQ 机器人**（`/admin/qq`）。此页独立检查权限，组织管理员和组织 API 密钥不能修改站点机器人设置。

- “机器人连接”：选择 NapCat 或 LLBot，设置总开关、HTTP 地址、机器人 QQ 号、API 访问密钥及事件签名密钥。页面根据架构显示对应配置说明和官方文档。
- “服务器与积分规则”：选择现有服务器，设置授权数字群号、单服开关、暖服人数阈值与每分钟积分、投票价格和时长、友方广播价格、预留位价格与小时数。
- 候选地图可以手填真实 ID，也可点击“读取游戏地图目录”后勾选 2–10 张。
- 点击“保存配置”后写入数据库，无需改源码或重启。机器人在下一轮读取，Worker 通常十秒内读取。已开始的操作可能完成，未来购买使用新的规则。
- 密钥使用站点 `ENCRYPTION_KEY` 加密保存；网页和 API 只返回是否已设置。留空保留，勾选清除才删除。总开关停用时仍可保存配置，积分和历史订单保留。
- “检测已保存的连接”只调用 `get_login_info` 并核对 QQ 号，不向群发消息。还需群内 `/帮助` 验证事件上报。
- 同时编辑会返回版本冲突；重新加载后再修改，避免覆盖别人刚保存的配置。

未在网页保存前使用环境变量作为兼容默认值；首次网页保存后数据库配置优先。修改环境变量不会覆盖已保存的网页配置。

管理接口（仅站点所有者网页会话，写请求携带 `X-Requested-With: warcon`）：

| 请求 | 用途 |
| --- | --- |
| `GET /api/admin/qq` | 返回脱敏配置、`revision` 和密钥是否已设置 |
| `PUT /api/admin/qq` | 保存完整配置：`revision, provider, enabled, url, selfId, policies`；`provider` 为 `napcat` 或 `llbot`；`token`、`secret` 留空保留；`clearToken`、`clearSecret` 明确清除 |
| `POST /api/admin/qq` | 检测已保存连接与登录账号 |

配置存入现有 `site_settings` 的 `qqCommunity` 项，无需新增迁移。环境文件和数据库备份都要保留 `ENCRYPTION_KEY` 才能解密。

旧配置未保存 `provider` 时默认为 NapCat；旧 API 客户端省略 `provider` 时保留当前选择。环境变量 `QQ_BOT_PROVIDER=napcat|llbot` 只提供首次保存前的默认架构。每个站点当前配置一个机器人连接，可以选择任一框架；切换框架不会迁移或清空积分、Steam 绑定、群规则和订单。相同 QQ 用户继续使用 `ob11:QQ号` 身份；切换机器人 QQ 号后，旧 QQ 号的待处理指令会过期，避免由新机器人执行。

## VIP 设置

同一 `/admin/qq` 页面下方新增独立的 VIP 区域。站点所有者选择游戏服务器，输入 **17 位 SteamID64**，勾选一个或多个权益并点击“保存 VIP 名单”。支持启用、停用、移除与备注；同服同 SteamID 不可重复，不需要该玩家注册网页或绑定 QQ。VIP 独立于 QQ 机器人总开关，默认不会自动创建任何 VIP。

| 权益 | 实际效果 |
| --- | --- |
| 预留位 | 加入该服预留位的期望名单，由现有 Worker 同步；停用或删除后只撤回不再被其他来源授予的席位。游戏版本可能要重启才能加载预留位配置 |
| 允许超杀 | 豁免数值限制的 KD、KPM（击杀/分钟）；单独勾选时，金钱增长限制仍有效 |
| 白名单 | 豁免数值限制、自动风险踢人和 Integrity 自动处罚；现有自动隔离在白名单有效期间不再阻止进服。数据记录、风险展示和人工审核保留 |

白名单不删除历史处罚，也不自动解除人工封禁、人工审核处罚或游戏原生封禁；这些仍由管理员单独处理。移除白名单后，尚未到期的自动隔离恢复执行。它不是管理员权限，也不替代预留位：满服排队优先权需单独勾选“预留位”。

- 管理：`GET /api/admin/qq/vips` 返回 `{ok,vips:{revision,entries}}`；`PUT` 提交 `{revision,entries}`，仅站点所有者会话，写请求带 `X-Requested-With: warcon`。条目字段为 `serverId, steamId, enabled, reserve, allowOverkill, whitelist, note`。版本冲突返回 409。
- 查询：`GET /api/servers/{serverId}/community?view=vip&steamId=...`，要求对应服务器 `server.view` 权限；返回有效 VIP 与三个权益，不公开管理员备注。
- 独立存储于 `site_settings.qqVip`，无需新增迁移。保存与名单/处罚执行分离，API 保存成功不代表游戏已确认预留位生效。所有修改写入审计记录。

## 配置并启动

### LLBot 接入

1. 按 [LLBot 官方安装文档](https://luckylillia.com/guide/choice_install)启动 LuckyLilliaBot，登录机器人 QQ 并加入授权群。
2. 在 WARCON 的 **站点管理 → QQ 机器人 → 机器人架构** 选择 **LLBot（LuckyLilliaBot）· OneBot 11**，保存接口地址、QQ 号和两个独立随机密钥，以及服务器和群规则。
3. 在 LLBot 的 Bot 配置中启用 **OneBot 11**，添加两项连接：
   - `http`：启用；同机示例监听 `127.0.0.1:3002`，`token` 填 WARCON 的 API 访问密钥；WARCON 地址相应填写 `http://127.0.0.1:3002`。端口可配置，须避开 WARCON 本身的端口。
   - `http-post`：启用；`url` 填管理页显示的 `/api/qq/webhook` 上报地址，`token` 填 WARCON 的事件签名密钥；`messageFormat=array`，关闭 `reportSelfMessage` 和 `reportOfflineMessage`，关闭 `debug`。
4. [llbot-onebot.example.json](llbot-onebot.example.json) 提供 `ob11` 部分配置，替换占位密钥和地址后合并到 LLBot 现有配置；不要覆盖其他配置项。LLBot 通常在 `bin/llbot/data/config_<QQ号>.json` 保存配置，也可直接使用其 WebUI 修改。
5. 点击 WARCON “检测已保存的连接”，再在授权群发送 `/帮助`。API 请求使用 `Authorization: Bearer <API密钥>`；HTTP 上报 token 生成 `X-Signature: sha1=...`，原始请求体和 `X-Self-ID` 必须由代理完整保留。未签名事件会被拒绝。

LLBot 接入复用下文所有群指令、战绩 API、举报、暖服积分、投票、友方广播和优先队列业务。这里对接 OneBot 11 HTTP + HTTP POST；没有启用 Milky、Satori、WebSocket 或任意 API 透传。开发依据：[LLBot 配置文档](https://luckylillia.com/guide/config)、[协议开发对接](https://luckylillia.com/guide/develop)、[HTTP 签名与 API 源码](https://github.com/LLOneBot/LuckyLilliaBot/blob/main/src/onebot11/connect/http.ts)。

### NapCat 接入

1. 按 [NapCat 安装文档](https://napneko.github.io/guide/boot/Shell)安装并启动 NapCat，用手机 QQ 扫码登录要作为机器人的账号。WARCON 不接收 QQ 密码。NapCat 账号必须已加入授权群。
2. 部署 WARCON 分支并运行现有数据库迁移。账本仍用 `0068_qq_community`，本次协议切换没有新增迁移。
3. 推荐使用上述网页配置。若暂不使用管理页，也可设置 WARCON 服务环境变量。Web 和 Worker 的 `QQ_BOT_POLICIES` 必须相同；`groups` 改为数字群号字符串，地图为实际游戏地图 ID。

```dotenv
QQ_BOT_PROVIDER=napcat
ONEBOT_HTTP_URL=http://127.0.0.1:3001
ONEBOT_ACCESS_TOKEN=生成独立随机长密钥A
ONEBOT_EVENT_SECRET=生成独立随机长密钥B
ONEBOT_SELF_ID=机器人QQ号
QQ_BOT_POLICIES='[{"serverId":"WARCON服务器ID","groups":["123456789"],"lowAt":20,"pointsPerMinute":1,"voteCost":10,"broadcastCost":20,"reserveCost":120,"reserveHours":24,"voteSeconds":120,"maps":["实际地图ID1","实际地图ID2"]}]'
```

四个 `ONEBOT_*` 字段同时留空表示停用；部分填写会启动失败，防止无鉴权运行。远程 HTTP API 要使用 HTTPS；同机允许回环 HTTP。密钥只放服务环境，不放 Git。旧 `QQ_BOT_APP_ID` 和 `QQ_BOT_SECRET` 不再读取。

4. 在 NapCat WebUI 的网络配置中创建：
   - **HTTP 服务端**：启用，监听 `127.0.0.1:3001`，token 填密钥 A；这是 WARCON 调用 NapCat 的接口。
   - **HTTP 客户端**：启用，上报 `https://你的WARCON地址/api/qq/webhook`，token 填密钥 B，`messagePostFormat=array`，`reportSelfMessage=false`；这是 NapCat 向 WARCON 上报事件。同机也可用 `http://127.0.0.1:3000/api/qq/webhook`。
   - NapCat HTTP 客户端的 token 实际用于生成 `X-Signature: sha1=...`，不是 Bearer。反向代理须保留 `X-Signature`、`X-Self-ID` 和原始请求体。示例见 [napcat-onebot.example.json](napcat-onebot.example.json)，替换占位值后再使用。
5. 重启 WARCON，在授权群发送 `/帮助`。支持直接发送或 `@机器人 /帮助`；命令必须以 `/`、`!` 或 `！` 开头。只处理普通群消息，忽略私聊、匿名、机器人自身和非文本消息。非授权群不查询也不回复。
6. `/绑定` 会给出网页地址和一次性码。登录 WARCON、验证 Steam 后在 `/qq-link` 输入本人发起的码，五分钟有效。右上角用户菜单有“QQ 机器人”入口。新 QQ 身份以 `ob11:QQ号` 保存，不能沿用旧官方 openid 绑定；已有旧绑定需在网页解绑后重绑，Steam 积分保留。

启用一个群意味着允许群成员查询该服战绩、在线名单和地图。普通群成员无法调用管理员 API、修改价格或冒用其他 Steam 积分。

## 指令与经济规则

| 指令 | 行为 |
| --- | --- |
| `/战绩 [玩家名或SteamID64]`、`/总结 [玩家]` | 仅汇总本服已保存的场次、击杀、死亡、胜负、连杀及最近作战记录；省略目标时查询绑定的本人 |
| `/举报 SteamID64 原因` | 使用已验证 Steam 身份，进入现有风控举报流程，冻结前后证据并交人工审核，不直接封禁 |
| `/服务器` | 当前服务器名称、地图、在线人数和容量 |
| `/局势 [页码]`、`/对局 [页码]`、`/比分 [页码]` | 三方实时比分、胜利进度、阵营人数和全部在线玩家的分页名单；25 人一页，以 🟥🟦🟩 色块表示阵营 |
| `/在线 [页码]` | 实时在线玩家名、SteamID 和阵营，20 人一页 |
| `/地图` | 本服候选地图及投票价格 |
| `/流水` | 本人最近 10 笔积分变动 |
| `/积分` | 查询当前余额和有效暖服分钟数 |
| `/发起投票` | 对管理员配置的候选地图开启全服唯一投票，默认 120 秒；结束后至少冷却五分钟 |
| `/投票`、`/投票 编号` | 查看投票或消费积分投票；每轮每个 Steam 一票，同服多个群共用同一轮；平票按配置的候选顺序决定 |
| `/友方广播 正文` | 消费积分后，用实时名单确认发起者阵营，将单人消息逐个发给当时同阵营玩家，包含本人 |
| `/优先队列` | 兑换默认 24 小时的服务器预留位，复用 WARCON 的名单同步与到期撤销 |
| `/订单` | 查看个人最近订单，以及本服地图设置结果 |
| `/解绑` | 提示前往网页解绑；积分仍属于 Steam 账号 |

默认价格只是可修改的初始值：1 积分/暖服分钟，10 积分/投票，20 积分/友方广播，120 积分/24 小时预留位。没有现金充值或积分转账。

`/局势` 的胜利进度按实时分数除以游戏返回的 `scoreCap` 计算，每个色块代表 10%，未满部分以 `▫️` 表示；文字百分比保留整数。比分超过目标时进度显示 100%，原始比分仍保留。未知比分或获胜目标标明未知，不以三方比分之和代替目标。阵营颜色优先读取游戏实际颜色，未加入阵营的玩家用 ⬜ 标记。名单按阵营分组，并以 `/局势 2` 等查询后续页，避免 QQ 长消息被截断；`/帮助` 列出所有指令。

暖服由 Worker 的可信连续在线观测自动累计，人数不超过 `lowAt` 时发放，分钟不足部分结转。不要求填满服务器；不依赖旧的 `seed_reward` 规则；不追溯历史暖服时长。不可信断线区间或超过 60 秒的观测间隔不记分。开启配置后的在线玩家可先积累积分，之后再绑定 QQ。积分入账与服务器观测在同一事务中，重复观测不重复入账；停用 QQ 配置时停止新增积分。

广播购买时固定收件人，发送前逐人重新检查发起者和收件人是否仍在线、仍属原阵营；离线、换边或超过两分钟的消息跳过。游戏没有原生的阵营定向广播事务，因此名单查询与发送之间仍有短暂竞态；不能保证零延迟的阵营一致性。每个收件人单独记录成功、跳过或未知结果。消息在游戏里的样式取决于单人消息接口，不承诺全屏显示。

预留位是现有 RCON 已支持的接入资格，并不等于经验证的队列排序接口。若游戏只在重启时加载配置，或服务器预留槽位数量为零，资格可能不能立即发挥作用；应先按原项目的预留位文档验证游戏配置。重复兑换有效名单中的账号会被拒绝，不重复扣分。

投票开始前检查实时轮换能力及地图目录；不支持时不收取投票积分。投票截止后将胜出地图写入下一张地图，不立即强制切图。两分钟后仍未执行的地图订单停止执行，交人工处理，避免长时间停机后的旧指令修改新比赛。

## 管理与集成 API

继续使用 WARCON 的组织 API 密钥：`Authorization: Bearer <组织API密钥>`；网页会话的写请求仍需 `X-Requested-With: warcon`。只有 QQ 回调使用独立的签名认证。

基础路径：`/api/servers/{serverId}/community`。

| 请求 | 内容与权限 |
| --- | --- |
| `GET ?view=status` | 实时服务器状态；`server.view` |
| `GET ?view=players&page=1` | 在线玩家分页；`server.view`，页码 1–999，每页 20 人 |
| `GET ?view=battle&page=1` | 实时阵营比分、获胜目标、比赛时间及 QQ 色块消息 `text`；`server.view`，每页 25 位玩家，返回总人数与总页数 |
| `GET ?view=maps` | 配置的候选地图及票价；`server.view` |
| `GET ?view=player&target=SteamID或名字` | 本服结构化战绩、最近场次、文字总结；`server.view` |
| `GET ?view=vote` | 当前/最近地图投票；`automation.manage` |
| `GET ?view=economy&steamId=SteamID` | 余额、最近 100 笔流水、30 笔订单、500 条逐人发送结果；`automation.manage` |
| `POST {"action":"friendly","steamId":"...","message":"...","requestId":"至少16位唯一键"}` | 兑换友方广播；`automation.manage` 与 `chat.send` |
| `POST {"action":"reserve","steamId":"...","requestId":"至少16位唯一键"}` | 兑换预留位；`automation.manage` 与 `slots.manage` |
| `POST {"action":"openVote"}` | 开启地图投票；`automation.manage` 与 `match.control` |
| `POST {"action":"vote","steamId":"...","choice":"1"}` | 为指定积分账户投一票；`automation.manage` 与 `match.control`。此为可信管理员/集成接口，不给普通群成员开放 |
| `POST {"action":"reconcile","orderId":"...","refund":true}` | 对 `unknown`/`partial`/`failed` 订单全额退款，最多一次；`automation.manage` |
| `POST {"action":"reconcile","orderId":"...","refund":false}` | 人工核实已兑现后关闭待核实订单；`automation.manage` |

购买请求的 `requestId` 仅允许 16–80 位字母、数字、下划线和连字符；相同账号、服务器、请求键返回原订单，不能重复扣款。价格由服务器配置确定，客户端不能自行指定。积分以服务器和 SteamID 隔离。

**失败与退款：** 先写订单，再进行 RCON 操作。服务中断或网络超时可能意味着游戏已执行，因此不会自动重试发送或自动退款。管理员查看逐人结果及游戏状态后做人工结算。地图订单退款会退回该轮所有已扣的投票积分；广播部分成功可由管理员选择全额退款，当前版本没有按收件人数部分退款。QQ 消息回复失败不撤销已完成的业务操作，用户可再次查询 `/积分`、`/投票`、`/订单`。

后台通过数据库锁串行处理，重复 QQ 事件仅保存一次，每个 QQ 号每分钟最多接收十条指令，事件时间须在五分钟内。回调先持久化再确认，处理与网络发送由后台完成。未知投递结果不会自动重发；不会把内部异常或密钥回显到 QQ。

## 验证与上线边界

- 自动测试覆盖签名、过期/篡改请求、Steam 验证、重复回调、暖服事务回滚、并发扣款、投票扣款幂等、候选胜出、退款幂等及友方收件人切换。
- 本地测试不向真实 QQ 群或游戏服发送消息。上线必须使用测试群验证回调、回复、战绩、举报、两阵营广播及预留位同步/到期撤销。
- 使用 `send_group_msg` 发送纯文本消息段，用户文本中的 CQ 码不会被解释为 @全体或其他操作。HTTP 200 之外还检查 OneBot `status`、`retcode` 和消息 ID；未知结果不自动重发。投票结果通过 `/投票` 或 `/订单` 查询。
- 现有历史数据不一定包含所有场次或所有击杀；文字总结是已记录数据的确定性汇总，不生成无法由数据证明的战术过程。

开发依据：[NapCat 网络配置](https://napneko.github.io/config/basic)、[NapCat 上报签名实现](https://github.com/NapNeko/NapCatQQ/blob/main/packages/napcat-onebot/network/http-client.ts)、[OneBot 11 HTTP](https://github.com/botuniverse/onebot-11/blob/master/communication/http.md)、[HTTP POST](https://github.com/botuniverse/onebot-11/blob/master/communication/http-post.md)、[群消息事件](https://github.com/botuniverse/onebot-11/blob/master/event/message.md)。游戏能力以[仓库的 RCON 接口记录](wardogs-api.md)和实际服务器返回为准。

## 接口调用示例

```sh
curl -H "Authorization: Bearer $WARCON_API_KEY" \
  "https://你的WARCON地址/api/servers/服务器ID/community?view=players&page=1"
curl -H "Authorization: Bearer $WARCON_API_KEY" \
  "https://你的WARCON地址/api/servers/服务器ID/community?view=player&target=76561198000000001"
```

现有举报 API 继续由原有权限控制，群内举报复用同一业务流程。本接口不提供任意 OneBot 方法透传，不暴露服务器密码、玩家 IP 或 QQ token。
