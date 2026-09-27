# 2026-09-27 Kill Feed 断流源码核查

## 结论与边界

检查版本：`643a84b`。截至 UTC 06:51（香港时间14:51），尚未发现最近委员会／AI修改导致接收接口拒绝或停住的证据。断流仍未恢复，游戏端发送模块或出站链路的具体原因尚未确认。

## 调用链检查

1. `src/hooks.server.ts`：`/api/ingest/` 路径豁免面板 JSON CSRF 自定义头要求，回传使用独立 Bearer 鉴权。
2. `src/routes/api/ingest/events/+server.ts`：令牌检查、限流、JSON校验，然后调用 `ingestBatch`。不调用外部 AI。
3. `src/lib/server/feed.ts`：同一服务器事务锁保护去重；击杀记录与 legacy／integrity 两个处理任务同事务提交。阵营未知或距离异常不删除整条有效击杀。
4. `src/lib/server/gateway-local.ts`：接收后唤醒后台消费，不等待风控计算完成。
5. `src/lib/server/feed-processing.ts`：两个 consumer 独立调度；失败保留任务重试。每个服务器同 consumer 保序；超时租约可恢复。
6. `src/lib/server/integrity/ai-queue.ts`：独立定时队列调用模型，受频率、预算和重试限制；不在击杀 HTTP 请求中调用。

与 `6b4deee` 比较，接收路由、Feed解析、Feed鉴权、入库、Feed消费、Gateway和请求Hook均无改动。近期委员会／AI更改位于入库后的评估链路。源码隔离本身不能排除共享数据库资源故障，因此同时检查了运行状态。

## 运行证据

- 线上追踪代码与 `643a84b` 一致，无跟踪文件改动；目录中有旧构建备份。
- 最后击杀入库时间：UTC 05:59:25.081；对应游戏服回传在 Nginx 返回200。
- 此前入口日志检查中，该次之后没有新的游戏服 Kill Feed HTTP 请求；配置地址与令牌匹配。
- UTC 06:51复查：玩家列表仍更新，99人在线；两条消费队列在最近4小时内均301批done，未返回pending／processing。
- 数据库应用连接为idle／ClientRead，检查时没有锁等待。
- 使用真实Feed令牌分别对本机HTTP和公网HTTPS发送非法空对象 `{}`，38ms／33ms返回预期400，错误为缺少 `events` 数组。这验证了鉴权、入口和格式校验可以响应；不等于对游戏服出站网络做了端到端验证。
- 探测没有伪造击杀，也没有刷新 `feed_at`；最后击杀与回传时间保持不变。
- 部署时曾在UTC05:48:26出现短暂上游连接失败。随后仍有正常200回传，直到05:59:25；故该次502不能直接证明本次持续断流由部署触发。

## 自动化测试

执行 feed-core、feed-auth-limit、feed、feed-ingest、feed-processing 五个测试文件，共32项通过、1439次断言，包括并发去重、隔离不同服务器、重启恢复、重复任务不重复处罚、消费者独立重试和阵营数据保护。

发现并修正旧测试夹具的问题：延迟／积压处罚测试原本要测试Legacy规则，却依赖默认Shadow模式；委员会KPM积分变化后已不满足原测试的处罚分。明确测试使用Legacy并清除配置缓存后，原有保留证据与禁止延迟处罚断言全部通过。没有调整生产处罚阈值。

## 后续所需证据

当前游戏接口把 `WDServerFeed` 标为 `next-restart`，没有暴露专门的Feed热重启接口。需要游戏进程UTC05:55–06:05及之后的Feed／HTTP／TLS运行日志，或托管端出站请求记录。RCON审计中的 `AUTH_OK` 和 `GET /v1/players` 不能证明游戏端正在发送击杀回传。
