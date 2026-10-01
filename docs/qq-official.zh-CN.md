# 腾讯官方 QQ 机器人

2026-10-01 已将 WARDOGS BOT（AppID 1905697626）接入 WARCON 的 OPEN1。程序直接使用腾讯 OpenAPI 和官方 WebSocket 网关，运行在现有 warcon.service 中，不依赖 QQ 客户端、LLBot、NapCat 或 OneBot 转发。

## 配置

站点所有者打开 `/admin/qq`，选择“腾讯官方 QQ 机器人 · OpenAPI”，接口地址固定为 `https://api.bot.qq.com`，填写 AppID 和 AppSecret。AppSecret 经现有 ENCRYPTION_KEY 加密保存；读取管理接口不返回明文。官方模式不需要填写 OneBot API token。

群规则必须使用 Tencent 事件的 `group_openid`，不是数字 QQ 群号。每个群只关联一台游戏服务器。未经管理员配置的群不执行业务；服务器日志仅记录其群 OpenID，方便管理员建立映射，不记录消息正文。

当前官方后台配置为 WebSocket，服务主动连接官方网关，处理心跳、断线重连和会话恢复。官方后台与接口的可用性仍由腾讯决定。也实现了 `/api/qq/webhook` 的 Ed25519 签名验证、op13 回调地址验证和 op12 确认；若改用 Webhook，还需要在腾讯后台配置有效 HTTPS 回调，并关闭 WebSocket 接收。

## 群内使用

在官方机器人所在群中发送 `@WARDOGS BOT /帮助`。可使用 `/服务器`、`/在线`、`/局势`、`/战绩 玩家名或SteamID64`、`/总结 玩家`。使用 `/绑定 SteamID64 游戏内昵称` 后可查询本人战绩、举报玩家、查询和兑换积分、地图投票、友方广播及预留位。

官方 OpenID 和旧数字 QQ 号属于不同身份命名空间，不能自动匹配。首次使用官方机器人需要重新绑定；现有 SteamID 下的积分账本保留。如同一 SteamID 仍被旧 QQ 身份绑定，需由原身份解绑或由管理员确认后处理，不能自动夺取绑定。

回复使用事件 d.id 作为 msg_id、固定 msg_seq=1。数据库以 AppID、群 OpenID 和原始消息 ID 去重，回调和网关收到相同消息不会重复执行。群指令仍限制每个身份每分钟十条，过期指令不执行。网络结果未知时不自动重发，避免重复购买或投递。

反作弊通知复用原有成功踢出后的持久化通知队列，包含原因、模型档位和 AI／规则／管理员来源。官方主动消息受腾讯权限、群接收设置和频率限制；接口成功确认前不能认定通知已经送达。

## 部署与维护

部署前备份数据库、原机器人配置、被改动源码和运行包，并检查远程源码未在构建期间变化。失败时恢复原运行包和配置。本次没有更改数据库结构或风控模式、阈值。

`journalctl -u warcon --since '10 minutes ago'` 可查看 `[qq-official] gateway ready`、连接错误或未配置的群 OpenID。凭证失败时检查 AppID、AppSecret 及腾讯后台 IP 白名单。

验证包括官方文档的挑战签名样例、原始字节签名和时效、token 缓存与业务错误码、官方群身份、被动/主动消息结构，以及真实数据库中的加密配置、签名回调、重复事件与回复结果。真实群接收情况需结合实际群消息与 qq_inbox.reply_state 核实。

官方依据：[访问凭证](https://bot.q.qq.com/wiki/develop/api-v2/dev-prepare/access-token.html)、[群消息事件](https://bot.q.qq.com/wiki/develop/api-v2/autogen/event/group_at_message_create.html)、[群消息发送](https://bot.q.qq.com/wiki/develop/api-v2/autogen/api/v2_groups_group_openid_messages.post.html)、[事件订阅](https://bot.q.qq.com/wiki/develop/api-v2/dev-prepare/interface-framework/event-emit.html)、[回调签名](https://bot.q.qq.com/wiki/develop/api-v2/dev-prepare/interface-framework/sign.html)。
