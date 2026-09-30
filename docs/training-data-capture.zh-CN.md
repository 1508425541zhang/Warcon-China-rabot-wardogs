# 模型训练原始数据采集

从迁移 `0067_training_raw_capture` 上线开始，保存经过 Feed 令牌认证、通过批次格式及大小检查的完整 JSON。原有 `kills`、案件与统计表继续提供业务查询；新增档案保留原始信息，方便重新构造模型特征。上线前没有保存的未知字段无法恢复。

## 数据保存在哪里

| 表／视图 | 内容 | 时间含义 |
|---|---|---|
| `training_feed_batches` | 完整 Feed 批次，包括所有字段、原始标签、未知类型、无法解析的单条事件、重复回传、空批次 | `received_at`：面板收到批次的 UTC 时间 |
| `training_feed_events`（只读视图） | 每一条原始事件；武器、爆头、穿透、源距离、事件时钟及完整 `raw_event` | `event_time_seconds`：游戏对局时钟，**不是 Unix 时间** |
| `training_observations` | 当前轮询实际收到的完整 `/v1/players` 和 `/v1/status` JSON，保留未来新增字段 | `poll_started_at`：轮询开始；`received_at`：收到该响应 |
| `kills` | 已解析击杀、击杀者／受害者、阵营来源、有效距离、武器、爆头、友伤、自杀、穿透等标签 | `ts` 为接收时间；`event_time` 为对局时钟 |

档案无自动过期清理，不跟随服务器条目级联删除。PostgreSQL 自动压缩较大的 JSONB（TOAST）；仍需按实际增长量扩容和备份。没有增加 RCON 请求频率，保存的是原有轮询取得的数据。只保存这两个游戏状态接口的响应，不保存 RCON 密钥、Feed 令牌或请求认证头。

每个 Feed 批次的原始档案、已解析击杀和消费任务在同一事务里写入。数据库写入失败时不会成功确认该请求，游戏可重试。重复回传在原始档案保留，业务击杀继续去重。事件档案包含游戏提供的 KO／伤害事件；如果游戏没有发出它们，不能根据击杀伪造伤害记录、射击数、命中部位或 KO。

## 原始与缺失语义

- 武器以 `cause` 原始 ID 保存；可据此在离线管线按武器分组，不能把模型缺少武器输入解释为已经按武器训练。
- `distance` 按游戏原始厘米值保留。`kills.distance_m` 是规范化米值；异常距离仍保存在原始档案，但训练前须标记无效，不能直接当成远距离作弊证据。
- `contextTags` 完整保存，包括 Headshot、Penetration、Ricochet 等标签。训练视图中的爆头／穿透在标签数组缺失时为 `NULL`，有数组但无对应标签时为 `false`。
- 原始玩家列表中没有 kills、deaths、cash 等字段时，JSON 中仍保持缺失；训练不要使用 UI 为展示而补出的零。
- `instance_id` 是游戏进程实例 ID；源 `matchId` 在部分游戏版本换图时不变化。离线分局还需结合地图、时钟回绕、`matches` 和玩家观察记录。
- 精确时间以源事件时钟为准。接收时间会受网络延迟和批次缓冲影响，不能把同批次共享的接收时间当作所有击杀同时发生。没有源 UTC 时间时，不宣称推算值就是精确发生时刻。

## 查询玩家原始事件

```sql
SELECT received_at, instance_id, source_match_id, source_map,
       event_id, event_type, event_time_seconds, weapon,
       raw_distance_cm / 100 AS source_distance_m,
       headshot, penetration, context_tags, raw_event
FROM training_feed_events
WHERE server_id = '替换为面板服务器ID'
  AND (killer_steam_id = '替换为SteamID' OR victim_steam_id = '替换为SteamID')
ORDER BY received_at, batch_id, event_index;
```

按 `(server_id, instance_id, event_id)` 去重有 ID 的事件，并检查同键不同内容的冲突。没有 ID 的事件保留 `(batch_id,event_index)` 及“无法可靠跨批次去重”标记，不能全部合并为一个空 ID。先去重再统计爆头率和 KPM，否则重传会放大击杀数。

## JSONL 导出

使用现有数据库连接环境，在项目目录执行：

```sh
bun scripts/export-training-data.ts 面板服务器ID /安全目录/新的导出目录
```

支持 `DATABASE_URL` 或 `PGHOST`、`PGPORT`、`PGUSER`、`PGPASSWORD`、`PGDATABASE` 环境变量。不要把密码放进 Git、公开说明或训练 JSON。

导出使用只读一致性快照，分页写出 `training_feed_batches.jsonl` 与 `training_observations.jsonl`，附 `manifest.json` 记录行数、SHA-256、快照时间。已有目录不会覆盖。每次是完整快照；重复导出按原始档案 ID 合并，避免递增 ID 与并发事务提交顺序不同而漏掉增量数据。导出目录只有产生完整 manifest 后才算完成。

训练关联案例时，继续使用现有 `integrity_cases`、`integrity_labels`、`integrity_actions`、`integrity_ai_reviews`、`audit_log`。区分人工确认、AI意见、统计规则和真实执行结果；它们不应自动混成作弊真值。原始日志是状态与事件数据，强化学习还需要明确动作、后续结果、奖励和评估方案。此采集功能不自动生成奖励，也不自动重训或修改线上处罚。

## 验收与增长监控

```sql
SELECT count(*), max(received_at), pg_size_pretty(pg_total_relation_size('training_feed_batches'))
FROM training_feed_batches;
SELECT endpoint, count(*), max(received_at) FROM training_observations GROUP BY endpoint;
SELECT event_type, count(*) FROM training_feed_events GROUP BY event_type;
```

检查最新时间持续前进，再对比磁盘 `df -h` 和数据库增长量。若没有游戏击杀，原始 Feed 表没有新击杀是正常现象；玩家／状态观察仍应随轮询增长。扩容必须在操作系统中确认可用空间增加，仅在服务商页面发起操作不代表文件系统已经扩容。
