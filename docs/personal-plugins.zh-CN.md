# Warcon China 个人插件开发与接入（API v1）

## 1. 先选一种方式

| 方式 | 适用情况 | 如何安装 |
| --- | --- | --- |
| JSON 卡片 | 想自己调整数据卡片、玩家表格、文字和强调色 | 登录后打开“个人插件”，导入 JSON，选择服务器，保存 |
| Svelte + TypeScript 组件 | 需要自定义布局、图表或交互 | 开发者把组件加入源码、注册并重新构建；用户在“个人插件”选用 |

两种方式都按账号保存配置，最多 20 个。插件不会获得额外服务器权限。中文界面沿用面板样式；代码组件使用同一套 Svelte 5、TypeScript、Tailwind CSS 4 和项目锁文件，不单独加载另一套 Svelte、React 或全局 CSS。

第一版提供展示插件，不提供上传 JS 后在服务器执行、动态安装 npm 包、Worker 后台任务或自动处罚挂钩。代码插件属于受信任的面板源码，部署者需审查；它不是沙箱。JSON 只能选择固定组件和样式字段，不执行 HTML、CSS 或 JavaScript。

## 2. 不写代码：一步一步安装 JSON 插件

1. 登录 Warcon China。
2. 点击顶部导航“个人插件”（小屏幕在账号菜单内）。
3. 点击“下载 JSON 示例”，或直接用“新建 JSON 示例”。
4. 如果已有文件，点击“导入 JSON”，选 UTF-8 编码的 `.json` 文件，最大 64 KB。
5. 修改插件名称和说明，选强调色、1～3 列以及舒适／紧凑间距。
6. 在“关联服务器”选择有权限查看的服务器；不选服务器时，静态文字可正常展示，数据指标显示“—”。
7. 在 JSON 的 `widgets` 中增删卡片，最多 12 个。标识 `id` 用 2～48 位小写字母、数字、连字符，并以字母开头。自己的两个插件不能使用同一个标识。
8. 若在 JSON 中修改了名称、说明、样式，点“应用 JSON 到表单”；保存以表单为准。
9. 保持“启用插件”勾选，点击“保存插件”。
10. 在上方“我的插件”点击“打开插件”。可见页面每 10 秒刷新，隐藏标签页暂停自动刷新。
11. 分享时点“导出 JSON”。导出的仅是清单和样式，不含服务器关联、登录会话或密钥；接收者需自行导入并选择服务器。

启停与删除只影响自己的配置。删除插件配置不会删除服务器，也不会卸载已编译的代码组件。代码组件需要由部署者从注册表移除、重新构建才能卸载。

## 3. 完整 JSON 示例

下载：`/examples/plugins/server-cards.json`。

```json
{
  "apiVersion": 1,
  "id": "my-server-cards",
  "name": "我的服务器卡片",
  "description": "在线玩家数据概况",
  "version": "1.0.0",
  "renderer": "cards",
  "style": { "accent": "#69d6e3", "columns": 2, "density": "comfortable" },
  "widgets": [
    { "type": "metric", "title": "当前在线", "metric": "online" },
    { "type": "metric", "title": "平均延迟", "metric": "averagePing" },
    { "type": "players", "title": "在线击杀榜", "columns": ["name", "faction", "kills", "deaths"], "limit": 10, "sortBy": "kills" },
    { "type": "text", "title": "管理员备注", "text": "数据只包含当前在线玩家。" }
  ]
}
```

### 字段约定

- `apiVersion`：目前只能填 `1`。`version` 是插件自身版本，格式 `1.0.0`。
- `renderer`：JSON 使用 `cards`；代码插件使用已注册组件的名字，如 `round-summary`。
- `style.accent`：六位十六进制颜色，例如 `#69d6e3`；`columns` 是 1、2、3；`density` 是 `comfortable` 或 `compact`。代码插件可选择使用这些参数，实际支持情况由组件决定。
- `metric` 数据卡片支持 `online` 在线人数、`kills` 在线玩家总击杀、`deaths` 在线玩家总死亡、`cash` 在线玩家总现金、`averagePing` 已知延迟的平均值（ms）。
- `players.columns` 支持 `name`、`steamId`、`faction`、`kills`、`deaths`、`cash`、`ping`。
- `players.limit`：1～50，默认 10；`sortBy` 支持 `kills`、`deaths`、`cash`、`ping`，降序，默认 `kills`。
- `text.text`：纯文本，最长 2,000 字符；不会解析 HTML。
- 名称和组件标题最多 80 字符，说明最多 500 字符。不接受未知字段、外部 URL、脚本或任意 CSS。

总击杀、总死亡、总现金来自当前在线玩家快照，不能解释为整场比赛总量；离线玩家不包含在内。现金是当前余额，不是赚钱效率。缺少数据时使用 JSON `null` 和界面“—”，不伪造为 0。玩家列表上限 256，超过上限会标记 `playersTruncated` 并将合计值与平均延迟设为 null。

## 4. 编写 Svelte + TypeScript 代码插件

项目已经包含可以直接启用的例子：

```text
src/lib/plugins/sdk.ts                       类型、JSON 校验、数据协议
src/lib/plugins/catalog.ts                   代码插件清单注册
src/lib/plugins/components.ts                Svelte 组件注册
src/lib/plugins/extensions/round-summary/
  manifest.json                             清单
  Plugin.svelte                             组件
```

### 4.1 创建文件

把 `round-summary` 文件夹复制为 `my-summary`。修改清单中的 `id` 和 `renderer` 为 `my-summary`，名称改成“我的概况”。保留 `apiVersion: 1`，代码组件可使用空的 `widgets: []`。

### 4.2 编写组件

下面代码可放进 `src/lib/plugins/extensions/my-summary/Plugin.svelte`：

```svelte
<script lang="ts">
  import type { PluginComponentProps } from '$lib/plugins/sdk';
  let { plugin, snapshot, loading, error }: PluginComponentProps = $props();
</script>

<section class="panel my-summary" style:--plugin-accent={plugin.style.accent}>
  <h2 class="text-xl font-semibold text-white">{plugin.name}</h2>
  <p class="mt-3">在线人数：{snapshot?.metrics.online ?? '—'}</p>
  <p>平均延迟：{snapshot?.metrics.averagePing ?? '—'} ms</p>
  {#if loading}<p class="text-mist-400">正在读取…</p>{/if}
  {#if error}<p class="text-warn">{error}</p>{/if}
</section>

<style>
  .my-summary { padding: 1.5rem; border-top: 3px solid var(--plugin-accent); }
</style>
```

组件通过 props 接收数据，不需要保存 API Key，也无需自行轮询。`snapshot` 在未关联服务器、权限失败或尚未加载时可以是 null。面板外层提供刷新按钮、过期提示及渲染错误边界。

### 4.3 注册清单和组件

在 `src/lib/plugins/catalog.ts` 增加导入，并把解析后的清单加进现有数组；保留原有示例：

```ts
import mySummary from './extensions/my-summary/manifest.json';
// codePluginCatalog 数组新增这一项：
pluginManifestSchema.parse(mySummary)
```

在 `src/lib/plugins/components.ts` 增加：

```ts
import MySummary from './extensions/my-summary/Plugin.svelte';
// pluginComponents 对象新增这一项：
'my-summary': MySummary
```

两处的 renderer 名字必须一致。仅上传一份 `renderer: "my-summary"` 的 JSON，不会把组件代码安装到服务器；未注册会返回 `unknown_renderer`。

### 4.4 保持语言和样式一致

- 所有面向管理员的文案使用中文，字段标识使用稳定英文，不随显示语言改名。
- 复用项目的 `panel`、`btn`、`input`、`text-mist-400` 等类。
- 组件 CSS 写在自身 `<style>` 中，让 Svelte 自动作用域隔离。避免 `:global(body)`、全局 `button` 覆盖和固定顶层遮罩。
- CSS 变量使用 `--plugin-` 前缀，不覆盖面板主题变量。
- 不额外加载 CDN 框架、全局样式重置或另一版本 Svelte。依赖随仓库锁文件统一构建。
- 不使用 `{@html}` 渲染玩家昵称或导入文本；不把密钥放入清单、组件代码或浏览器本地存储。
- 若增加自己的后端 API，必须使用现有认证、CSRF、服务器权限校验；前端隐藏按钮不能代替后端权限。

### 4.5 测试和部署

在项目根目录执行：

```sh
bun install --frozen-lockfile
bun run check
bun test src/test/personal-plugins.test.ts
bun run build
```

数据库测试需要 `TEST_DATABASE_URL` 指向允许创建临时数据库的测试 PostgreSQL；未配置会跳过数据库用例，不要把生产库作为测试库。部署使用项目现有流程，包含新的 `build`、源码及 `drizzle` 迁移目录，然后重启 Warcon。Docker 安装可按仓库 Compose 配置重新构建、启动。启动迁移 `0065_personal_plugins` 创建配置表。

登录新版本，打开“个人插件”，选择“使用：我的概况”，关联服务器并保存，即完成安装。

## 5. HTTP API

以下路径相对于 Warcon 面板域名；不直接访问游戏 RCON。成功返回 JSON 和 `ok: true`，失败为 `ok: false` 及 `error.message`、`error.code`。所有数据响应禁用缓存。

### 5.1 个人配置 API：使用登录会话

| 方法 | 路径 | 用途 |
| --- | --- | --- |
| GET | `/api/plugins` | 列出当前账号配置，返回 `plugins` |
| POST | `/api/plugins` | 安装配置，201，返回 `plugin` |
| GET | `/api/plugins/{pluginId}` | 查看本人配置 |
| PUT | `/api/plugins/{pluginId}` | 完整替换清单、关联服务器及启用状态 |
| DELETE | `/api/plugins/{pluginId}` | 删除本人配置 |
| GET | `/api/plugins/{pluginId}/data` | 读取已启用插件关联的数据，返回 `apiVersion: 1`、`snapshot` |

写请求使用 `Content-Type: application/json` 和 `x-requested-with: warcon`。需要浏览器登录 Cookie；组织 Bearer API Key 不能修改个人配置。每次读取服务器数据都重新检查权限。未启用插件的数据接口返回 409 `plugin_disabled`。未关联服务器返回 `snapshot: null`。

POST / PUT 请求格式：

```json
{
  "manifest": { "apiVersion": 1, "id": "my-card", "name": "在线人数", "renderer": "cards", "widgets": [{ "type": "metric", "title": "当前在线", "metric": "online" }] },
  "serverId": "替换为服务器编号，或填 null",
  "enabled": true
}
```

PUT 是完整替换，不是 PATCH；编辑时不能更换 `manifest.id`。想复制插件，使用新的 id 发 POST。同一账号 id 重复返回 409 `plugin_exists`；超 20 个返回 409 `plugin_limit`；格式错误返回 400 `invalid_plugin`；上传超过 64 KB 返回 413。

在面板源码里使用既有客户端，它自动携带会话与 CSRF 请求头：

```ts
import { api } from '$lib/api';
import type { PluginManifest, PluginSnapshot } from '$lib/plugins/sdk';

export async function installPlugin(manifest: PluginManifest, serverId: string) {
  return api('POST', '/api/plugins', { manifest, serverId, enabled: true });
}
export async function readPlugin(pluginId: string) {
  return api<{ snapshot: PluginSnapshot | null }>(
    'GET', `/api/plugins/${encodeURIComponent(pluginId)}/data`
  );
}
```

### 5.2 外部客户端：使用组织 API Key

读接口：`GET /api/servers/{serverId}/plugin-snapshot`。

创建有 `server.view` 权限、限定目标服务器范围的组织 API Key，在自己的后端保存；通过 `Authorization: Bearer ...` 请求。浏览器中运行的内置插件使用上面的会话接口，不把 Bearer Key 发给客户浏览器。

```sh
curl --fail-with-body \
  -H "Authorization: Bearer $WARCON_API_KEY" \
  "https://你的面板域名/api/servers/服务器编号/plugin-snapshot"
```

独立 Node / Bun 客户端示例：

```ts
const origin = process.env.WARCON_ORIGIN!;
const serverId = process.env.WARCON_SERVER_ID!;
const key = process.env.WARCON_API_KEY!;
const response = await fetch(
  `${origin}/api/servers/${encodeURIComponent(serverId)}/plugin-snapshot`,
  { headers: { Authorization: `Bearer ${key}` }, signal: AbortSignal.timeout(10_000) }
);
if (!response.ok) throw new Error(`Warcon API: ${response.status}`);
const { snapshot } = await response.json();
console.log(snapshot.metrics);
```

响应示例（说明结构用的示例数值）：

```json
{
  "ok": true,
  "snapshot": {
    "apiVersion": 1,
    "server": { "id": "server-id", "name": "社区服务器", "map": "Kavkazi" },
    "statusAt": "2026-09-27T08:00:00.000Z",
    "playersAt": "2026-09-27T08:00:00.000Z",
    "stale": false,
    "metrics": { "online": 1, "kills": 3, "deaths": 1, "cash": 100, "averagePing": 30 },
    "players": [{ "name": "玩家甲", "steamId": "76561198000000001", "faction": "Blue", "kills": 3, "deaths": 1, "cash": 100, "ping": 30 }],
    "playersTruncated": false
  }
}
```

`stale` 表示玩家快照缺失或超过 90 秒；`statusAt` 单独表示地图状态时间。数据来自 Worker 最近采样，调用这个接口不会强制游戏服务器重新采集。建议客户端至少间隔 10 秒、避免并发轮询，失败时停止显示旧数据并显示错误。缺字段或 `null` 应显示“—”。不返回 RCON 地址／密码、Feed 令牌、Steam Key 或 AI Key。

## 6. 常见问题

- **JSON 导入成功却没有数字**：先关联服务器，确认插件已启用、账号有权限，且 Worker 已采集玩家列表。
- **显示旧数据**：检查页面的快照时间和过期提示；这个插件接口不负责修复游戏连接。
- **代码插件找不到**：检查两个注册表的 renderer 是否一致，确认服务器已部署新构建并重启。
- **导入另一个人的 JSON 无权看他的服务器**：需要自行选择有权限的服务器，导入文件不授予权限。
- **样式没有变化**：JSON 卡片读取全部 style 字段；代码组件需自己使用相应 props，示例代码组件仅使用强调色。
- **名称改了没生效**：JSON 编辑后先点“应用 JSON 到表单”，再保存；优先级以表单为准。
