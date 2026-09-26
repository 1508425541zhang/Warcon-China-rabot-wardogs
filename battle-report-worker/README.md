# 战斗 Report — Cloudflare Worker 独立版

一个可以单独部署到 Cloudflare Workers 的轻量社区举报站。

## 包含什么

- 单 Worker 提供静态举报网页和 API
- Cloudflare D1 保存举报记录
- `POST /api/report` 创建举报
- `GET /api/reports` 管理员读取举报（Bearer Token）
- Cloudflare Email Service / Email Routing 接收来信
- 来信写入 D1，并使用 Email Worker reply 自动回执
- 网页举报成功后通过 `send_email` binding 给管理员发通知
- 无 React / Vue / Svelte，无独立服务器

## 1. 安装

```bash
cd battle-report-worker
npm install
```

## 2. 创建 D1

```bash
npx wrangler d1 create battle-report-db
```

把命令返回的 `database_id` 填进 `wrangler.jsonc`。

初始化数据库：

```bash
npm run db:migrate:remote
```

本地开发数据库：

```bash
npm run db:migrate:local
```

## 3. 配置管理员 Token

不要把 Token 写进 Git。

```bash
npx wrangler secret put ADMIN_TOKEN
```

之后管理员接口：

```bash
curl -H "Authorization: Bearer YOUR_TOKEN" \
  https://YOUR_WORKER.workers.dev/api/reports
```

## 4. 配置邮件

在 `wrangler.jsonc` 中修改：

- `REPORT_FROM_EMAIL`：你的 Cloudflare Email Service 域名下发件地址，例如 `report@example.com`
- `ADMIN_EMAIL`：管理员接收举报通知的地址

Cloudflare Email Service / Email Routing 中：

1. 启用你的域名邮件服务。
2. 创建一个邮件路由地址，例如 `report@example.com`。
3. 把该地址的目标设置为这个 Worker。
4. 确保 `ADMIN_EMAIL` 符合你的 Email Service / Email Routing 发件绑定权限。

收到邮件后 Worker 会：
- 解析主题和正文
- 写入 `inbound_emails`
- 自动回复“邮件已收到”
- 若主题里包含类似 `举报: PLAYER123`，仍然只作为来信保存，不自动处罚任何玩家

## 5. 本地开发

```bash
npm run dev
```

Email Worker 可以通过 Wrangler 的本地 Email Worker 测试入口进行测试。

## 6. 部署

```bash
npm run deploy
```

部署成功后打开 Workers 返回的网址即可直接使用举报页面。

## API

### POST /api/report

```json
{
  "playerId": "SteamID64 / 玩家ID / 玩家名",
  "category": "作弊疑似",
  "details": "具体发生了什么",
  "reporterEmail": "optional@example.com"
}
```

### GET /api/reports

需要：

```
Authorization: Bearer <ADMIN_TOKEN>
```

支持查询参数：

- `limit`：1-100，默认 50
- `status`：例如 `new`、`reviewing`、`closed`

### PATCH /api/reports/:id

需要管理员 Token。

```json
{ "status": "reviewing" }
```

允许状态：`new`、`reviewing`、`closed`。

## 设计原则

举报只是待审核线索，不是处罚结论。这个项目不会因为举报次数自动封禁任何人。
