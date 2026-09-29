# 官方 QQ 机器人、暖服积分与兑换

此功能集成在 WARCON Web/API 和 Worker 中，共用 PostgreSQL，不需要另装 OneBot/NapCat。默认关闭。QQ群 `@机器人 /帮助` 查看命令，网页 `/qq-link` 绑定已验证的 Steam 账号。该接口尚需使用你的 QQ 应用和真实游戏服完成上线联调。

## 配置并启动

1. 在 QQ 开放平台创建有群聊权限的机器人，取得 AppID、AppSecret；在开发/沙箱群中先测试。
2. 更新代码和构建镜像，运行项目现有数据库迁移命令 `bun run db:migrate`，或使用 Docker Compose 的 migrate 服务。新增迁移为 `0068_qq_community`。备份数据库后按项目已有部署流程发布。
3. 在 `.env` 填写以下字段。`serverId` 是 WARCON 内部服务器 ID；`groups` 是 QQ 回调中的 `group_openid`，不是数字群号；地图必须使用游戏地图目录的真实 ID。Web 与 Worker 必须获得同一份 `QQ_BOT_POLICIES`。已有 Compose 的 `env_file` 会传入两边。

```dotenv
QQ_BOT_APP_ID=你的AppID
QQ_BOT_SECRET=你的AppSecret
QQ_BOT_POLICIES='[{"serverId":"WARCON服务器ID","groups":["群openid"],"lowAt":20,"pointsPerMinute":1,"voteCost":10,"broadcastCost":20,"reserveCost":120,"reserveHours":24,"voteSeconds":120,"maps":["实际地图ID1","实际地图ID2"]}]'
```

4. 在 QQ 平台配置 HTTPS 回调地址 `https://你的WARCON域名/api/qq/webhook`，订阅 `GROUP_AT_MESSAGE_CREATE`。反向代理须保留 `X-Bot-Appid`、`X-Signature-Ed25519`、`X-Signature-Timestamp` 和原始请求体。回调 URL 验证也要经过签名校验。
5. 将机器人加入所配置的群，按平台的发布/权限审核流程启用。启用某群意味着允许该群成员查询对应服务器的玩家战绩，并使用绑定后的社区功能；请只配置授权群。
6. 群内发送 `/绑定`，登录 WARCON 的 `/qq-link` 页面，输入五分钟有效的一次性码。必须先在 `/account` 验证 Steam；单独输入 SteamID 不能冒充本人。每服每个 Steam 只能绑定一个 QQ 身份。绑定码不包含登录凭据，但只能填写自己触发的码。

QQ AppSecret 只保存在服务环境变量中，不传给浏览器。机器人的群成员 openid 无法换算为 QQ 数字账号，不以群昵称作授权。

## 指令与经济规则

| 指令 | 行为 |
| --- | --- |
| `/战绩 [玩家名或SteamID64]`、`/总结 [玩家]` | 仅汇总本服已保存的场次、击杀、死亡、胜负、连杀及最近作战记录；省略目标时查询绑定的本人 |
| `/举报 SteamID64 原因` | 使用已验证 Steam 身份，进入现有风控举报流程，冻结前后证据并交人工审核，不直接封禁 |
| `/积分` | 查询当前余额和有效暖服分钟数 |
| `/发起投票` | 对管理员配置的候选地图开启全服唯一投票，默认 120 秒；结束后至少冷却五分钟 |
| `/投票`、`/投票 编号` | 查看投票或消费积分投票；每轮每个 Steam 一票，同服多个群共用同一轮；平票按配置的候选顺序决定 |
| `/友方广播 正文` | 消费积分后，用实时名单确认发起者阵营，将单人消息逐个发给当时同阵营玩家，包含本人 |
| `/优先队列` | 兑换默认 24 小时的服务器预留位，复用 WARCON 的名单同步与到期撤销 |
| `/订单` | 查看个人最近订单，以及本服地图设置结果 |
| `/解绑` | 提示前往网页解绑；积分仍属于 Steam 账号 |

默认价格只是可修改的初始值：1 积分/暖服分钟，10 积分/投票，20 积分/友方广播，120 积分/24 小时预留位。没有现金充值或积分转账。

暖服由 Worker 的可信连续在线观测自动累计，人数不超过 `lowAt` 时发放，分钟不足部分结转。不要求填满服务器；不依赖旧的 `seed_reward` 规则；不追溯历史暖服时长。不可信断线区间或超过 60 秒的观测间隔不记分。开启配置后的在线玩家可先积累积分，之后再绑定 QQ。积分入账与服务器观测在同一事务中，重复观测不重复入账；停用 QQ 配置时停止新增积分。

广播购买时固定收件人，发送前逐人重新检查发起者和收件人是否仍在线、仍属原阵营；离线、换边或超过两分钟的消息跳过。游戏没有原生的阵营定向广播事务，因此名单查询与发送之间仍有短暂竞态；不能保证零延迟的阵营一致性。每个收件人单独记录成功、跳过或未知结果。消息在游戏里的样式取决于单人消息接口，不承诺全屏显示。

预留位是现有 RCON 已支持的接入资格，并不等于经验证的队列排序接口。若游戏只在重启时加载配置，或服务器预留槽位数量为零，资格可能不能立即发挥作用；应先按原项目的预留位文档验证游戏配置。重复兑换有效名单中的账号会被拒绝，不重复扣分。

投票开始前检查实时轮换能力及地图目录；不支持时不收取投票积分。投票截止后将胜出地图写入下一张地图，不立即强制切图。两分钟后仍未执行的地图订单停止执行，交人工处理，避免长时间停机后的旧指令修改新比赛。

## 管理与集成 API

继续使用 WARCON 的组织 API 密钥：`Authorization: Bearer <组织API密钥>`；网页会话的写请求仍需 `X-Requested-With: warcon`。只有 QQ 回调使用独立的签名认证。

基础路径：`/api/servers/{serverId}/community`。

| 请求 | 内容与权限 |
| --- | --- |
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

后台通过数据库锁串行处理，重复 QQ 事件仅保存一次，每个成员每分钟最多接收十条指令。回调先持久化再确认，处理与网络发送由后台完成。未知投递结果不会自动重发；不会把内部异常或密钥回显到 QQ。

## 验证与上线边界

- 自动测试覆盖签名、过期/篡改请求、Steam 验证、重复回调、暖服事务回滚、并发扣款、投票扣款幂等、候选胜出、退款幂等及友方收件人切换。
- 本地测试不向真实 QQ 群或游戏服发送消息。上线必须使用沙箱群验证回调、回复、战绩、举报、两阵营广播及预留位同步/到期撤销。
- 官方文档说明主动推送自 2025-04-21 停止提供，因此这里只使用带 `msg_id`/`msg_seq` 的被动群回复；暖服自动计分不依赖主动通知。投票结果通过 `/投票` 或 `/订单` 查询。
- 现有历史数据不一定包含所有场次或所有击杀；文字总结是已记录数据的确定性汇总，不生成无法由数据证明的战术过程。

官方来源：[消息收发](https://github.com/tencent-connect/bot-docs/blob/main/docs/develop/api-v2/server-inter/message/send-receive/send.md)、[官方 Webhook 实现](https://github.com/tencent-connect/botgo/blob/master/interaction/webhook/webhook.go)。游戏能力以[仓库的 RCON 接口记录](wardogs-api.md)和实际服务器返回为准。
