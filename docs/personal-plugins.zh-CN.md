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
  "serverId": null,
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

## 7. 这套插件究竟由哪几层组成

开发前先分清三个对象。**清单（manifest）**说明插件叫什么、用哪个渲染器、要显示哪些卡片；它是可导出、可分享的 JSON。**个人配置（PersonalPlugin）**把清单绑定到一个账号，同时记录关联的服务器、启用状态、更新时间；它保存在 PostgreSQL，其他账号不会因为拿到清单而得到这份配置。**服务器快照（PluginSnapshot）**是面板最近一次采集并保存的玩家和地图数据；插件读取快照，不直接连接游戏服务器。

典型请求顺序如下：管理员登录 → 进入 `/plugins` → 浏览器向 `/api/plugins` 保存配置 → 打开 `/plugins/{pluginId}` → 页面向 `/api/plugins/{pluginId}/data` 取快照 → 渲染 `cards` 或已随程序构建的 Svelte 组件。打开的标签页约每 10 秒再次请求；标签页隐藏时暂停定时刷新。这里的 10 秒是**页面读取频率**，不是游戏服务器采样频率；连续刷新也不会凭空产生新的游戏事件。

另一条路径给独立后台程序：组织管理员创建只含 `server.view` 的 API Key → 程序以 Bearer 方式 GET `/api/servers/{serverId}/plugin-snapshot` → 程序自己展示或分析。这个接口与个人配置无关，不需要先创建个人插件。建议把这种客户端运行在你控制的服务器，不要把 Key 写进公开网页或安装包。

当前 API v1 的能力边界也要明确：只有只读快照、卡片、文字、玩家表格，以及经过源码审核并编译的 Svelte 组件。清单不能定义自己的 URL、数据库查询、RCON 命令、后台定时任务或任意脚本。`renderer` 不是从 npm 仓库动态下载的包名；它必须匹配面板已经注册的组件。希望展示一局历史击杀、完整案件证据或新的统计图，需要单独设计有权限校验的数据接口，并在源码中开发，不可假定本接口已经返回这些字段。

### 7.1 代码位置和维护责任

| 文件或目录 | 作用 | 修改时机 |
| --- | --- | --- |
| `src/lib/plugins/sdk.ts` | 清单校验、字段枚举、组件 props、快照类型 | 扩展协议字段时修改；改动需考虑兼容性 |
| `src/lib/plugins/Cards.svelte` | 通用 JSON 卡片渲染 | 改卡片的统一展示方式时修改 |
| `src/lib/plugins/catalog.ts` | 编译进程序的代码插件清单 | 增加或移除代码插件时修改 |
| `src/lib/plugins/components.ts` | `renderer` 到 Svelte 组件的映射 | 增加或移除代码插件时修改 |
| `src/lib/plugins/extensions/` | 代码插件示例和新组件 | 开发受信任组件时添加目录 |
| `src/lib/server/personal-plugins.ts` | 个人配置校验、权限检查、快照裁剪 | 改 API 语义时修改，并同步测试 |
| `src/routes/api/plugins/` | 个人配置 HTTP 路由 | 扩展个人 API 时修改 |
| `src/routes/api/servers/[id]/plugin-snapshot/` | 会话或组织 Key 的快照 HTTP 路由 | 扩展外部读取接口时修改 |
| `src/routes/(app)/plugins/` | 配置页和插件运行页 | 改交互与刷新行为时修改 |
| `drizzle/0065_personal_plugins.sql` | 数据库建表迁移 | 只在首次引入该功能时运行 |

`docs/personal-plugins.zh-CN.md` 是本仓库中的开发手册。文档中的路径从仓库根目录计算。开发者应以当前分支的 `sdk.ts`、路由和权限代码为最终依据；版本号 `apiVersion: 1` 只描述这一版清单和快照协议，并不承诺以后永远不变。

## 8. 清单协议逐字段说明

最外层必须是 JSON 对象。服务端用严格校验：拼错字段名、传入未定义字段、把数字写成字符串，都会返回 400 `invalid_plugin`。先在本地用 `pluginManifestSchema.parse()` 校验，错误会标明字段路径。`id` 与 `renderer` 使用同一字符规则：2～48 位，第一个是小写英文字母，其后只允许小写英文字母、数字、连字符。例如 `team-report` 合法，`Team_Report`、`1-report` 不合法。

| 字段 | 类型和约束 | 省略后的值 | 用法 |
| --- | --- | --- | --- |
| `apiVersion` | 数字，必须恰为 `1` | 无，必须提供 | API 协议版本 |
| `id` | 2～48 位规则化字符串 | 无，必须提供 | 个人插件唯一标识；保存后不可更改 |
| `name` | 去掉首尾空白后 1～80 字符 | 无，必须提供 | 页面标题和插件列表名称 |
| `description` | 字符串，最多 500 字符 | 空字符串 | 给管理员看的说明 |
| `version` | `数字.数字.数字`，例如 `2.1.0` | `1.0.0` | 开发者维护的插件版本；不是自动升级器 |
| `renderer` | 符合 id 规则，且已注册 | `cards` | 选择通用卡片或已编译组件 |
| `style` | 对象，字段见下表 | 默认颜色、2 列、舒适 | 通用卡片会读取；代码组件自行决定是否读取 |
| `widgets` | 数组，最多 12 个 | 空数组 | `cards` 至少需要 1 个；代码组件可为空 |

`style` 若整个对象省略，三项都采用默认值；若显式写 `style`，其中各字段也可省略。`accent` 必须是带 `#` 的六位十六进制颜色，例如 `#69d6e3`；不接受 `red`、`#fff`、`rgb(...)`。`columns` 必须是整数 1、2、3，默认 2；`density` 只接受 `comfortable` 或 `compact`，默认 `comfortable`。这些参数是展示参数，不是权限或数据过滤条件。

每个 `widgets` 项用 `type` 选择一种结构，不可以把另一种结构的字段混进来：

| 类型 | 必填字段 | 可选字段 | 数据含义 |
| --- | --- | --- | --- |
| `metric` | `type`, `title`, `metric` | 无 | 展示一个快照合计值 |
| `text` | `type`, `title`, `text` | 无 | 展示静态纯文本 |
| `players` | `type`, `title`, `columns` | `limit`, `sortBy` | 展示在线玩家表格 |

三种卡片的 `title` 都是 1～80 字符。`metric` 允许 `online`、`kills`、`deaths`、`cash`、`averagePing`，拼写区分大小写。它们分别是当前玩家快照的在线人数、在线玩家击杀合计、在线玩家死亡合计、在线玩家现金余额合计和已知 ping 的平均值。`text.text` 最多 2,000 字符，按纯文本呈现；Markdown、HTML 和脚本不会被解释执行。`players.columns` 必须有 1～7 项，每项只能从 `name`、`steamId`、`faction`、`kills`、`deaths`、`cash`、`ping` 中选择。当前校验没有要求列名互不重复，所以开发时应自行去重，避免出现两个同名列。`players.limit` 为整数 1～50，默认 10；`sortBy` 只能是 `kills`、`deaths`、`cash`、`ping`，默认 `kills`，通用卡片按该项降序展示。

三个常见误解：`kills` 是玩家列表里当前在线者的数值合计，不能称为整场总击杀；`cash` 是余额合计，不是本场净收入；`averagePing` 只对有有效 ping 的玩家求平均，不能推测 IP 或地区。空值 `null` 与数字 `0` 不同，前者说明目前不能可靠给出值。不要把 `null` 用 `Number(value) || 0` 转为零。

### 8.1 三种最小清单

下面这个 JSON 是能在无服务器关联时看到的最小静态插件：

```json
{
  "apiVersion": 1,
  "id": "notice-board",
  "name": "值班说明",
  "renderer": "cards",
  "widgets": [
    { "type": "text", "title": "交班事项", "text": "请先核对服务器时间和玩家编号。" }
  ]
}
```

只显示在线人数的配置如下；未关联服务器时卡片出现“—”是预期行为：

```json
{
  "apiVersion": 1,
  "id": "online-count",
  "name": "在线人数",
  "widgets": [
    { "type": "metric", "title": "当前在线", "metric": "online" }
  ]
}
```

代码插件清单可以没有 `widgets`，但 `renderer` 必须事先注册并编译：

```json
{
  "apiVersion": 1,
  "id": "my-summary",
  "name": "我的概况",
  "renderer": "my-summary",
  "widgets": []
}
```

### 8.2 导入文件和 HTTP 请求体不是同一层

`/plugins` 页面“导入 JSON”选择的是**清单对象本身**，即文件顶层直接有 `apiVersion`、`id`、`widgets`。直接调用 `POST /api/plugins` 时，请求体顶层却必须有 `manifest`、`serverId`、`enabled`，其中 `manifest` 才是上面的清单。把导出的文件原样 POST 到 API，会因为缺少 `manifest` 而得到 400。反过来，把 API 请求体当作导入文件，也会校验失败。

`serverId` 可用目标服务器 ID 或 JSON `null`，省略时默认 `null`；不要写字符串 `"null"`。`enabled` 可用 `true` 或 `false`，省略时默认 `true`。整个请求体不允许额外字段。导出文件有意不包含 `serverId` 和 `enabled`，接收方需自己选择服务器、启停状态。浏览器页面最多允许导入 65,536 字节的文件；API 对整个 POST/PUT JSON 请求体使用同样的字节上限。

## 9. 网页内操作：从零到能看到数据

1. 先登录面板。若账号被要求更改密码或补全登录方式，先按账号页面提示完成；未完成时相关 API 可能返回 403。
2. 在浏览器地址栏输入面板的 `/plugins`，例如 `https://你的面板域名/plugins`。这里是插件配置页，不是公开访问的服务器展示页。
3. 点击“新建 JSON 示例”，或者下载并导入 `static/examples/plugins/server-cards.json`。如果用文本编辑器修改文件，保存为 UTF-8 的 `.json`，不要加 JavaScript 注释或尾随逗号。
4. 在“关联服务器”下拉框选择要读取的服务器。列表只包含当前账号有 `server.view` 权限的服务器。只想展示静态文字，可以选“不关联”。
5. 在 JSON 编辑框增删 `widgets`。点“应用 JSON 到表单”，让名称、说明、强调色和列数也从 JSON 更新；也可以直接在表单中改这些字段。点击“保存插件”时，名称、说明和样式**以表单为准**，其余字段仍从编辑框校验。
6. 回到“我的插件”列表，点击“打开插件”。若显示“尚未关联服务器”，回去选服务器；若显示“数据已过期或尚未采集”，检查 Worker 和服务器玩家快照采集。刷新按钮只重新读已存储的快照。
7. 需要转给另一位管理员时点“导出 JSON”。对方导入后，必须用自己的账号选择有权访问的服务器。不要把登录 Cookie、组织 Key 或其他密钥放进清单。

“停用”通过 PUT 保存 `enabled: false`；停用后插件运行页显示停用，数据接口返回 409。再次启用不会改变清单。删除需要在页面二次确认，删除的是当前账号保存的配置；已编译的代码组件仍留在构建中，其他人的个人配置也不会被删除。

## 10. HTTP 调用约定、认证与请求头

所有路径以**面板 HTTPS 域名**为基础，例如 `https://你的面板域名/api/plugins`。示例中的“你的面板域名”“服务器编号”和令牌都要替换成真实值。服务器 ID 可从面板服务器页 URL `/server/{id}/...` 取出；不要把 SteamID 当作服务器 ID。`pluginId` 取清单 `id`，作为路径段时先做 URL 编码。

| 调用者 | 用途 | 必要认证 | 写请求的额外头 |
| --- | --- | --- | --- |
| 已登录的同源浏览器 | `/api/plugins` 全部个人配置接口 | 面板登录 Cookie；`fetch` 使用 `credentials: 'same-origin'` | `X-Requested-With: warcon`，有 JSON 请求体时再加 `Content-Type: application/json` |
| 已登录的同源浏览器 | `/api/servers/{id}/plugin-snapshot` | 面板登录 Cookie 和 `server.view` | 此接口只有 GET，不写入 |
| 受控的独立后台程序 | `/api/servers/{id}/plugin-snapshot` | `Authorization: Bearer wck_...`，组织 Key 含 `server.view` 且范围覆盖服务器 | 此接口只有 GET；无需 CSRF 头 |

个人配置接口不会因为一个组织 Key 具有 `server.view` 就允许创建或修改个人配置。`POST`、`PUT`、`DELETE` 等 JSON API 写请求，如果没有有效的组织 Key，统一需要值**恰好**为 `warcon` 的 `X-Requested-With` 头；缺失时返回 403 `csrf`。个人插件写入还需登录会话。GET 不要求 CSRF 头，但仓库的浏览器 `api()` 工具会统一携带它。`Accept: application/json` 可作为客户端偏好发送，当前实现不依赖它做内容协商。`Content-Type: application/json; charset=utf-8` 可被当前接口识别；写请求不要发 `multipart/form-data`。

组织 Key 的实际格式以 `wck_` 开头；它与 Steam Key、Kill Feed 令牌、AI 模型 Key 都不是同一种凭据。服务端只接受格式正确、未过期、未撤销的组织 Key。Key 的服务器范围必须包含目标服务器，且其组织与服务器组织相同。带有 `wck_` Bearer 的 API 请求会按该 Key 解析，不会再借用同时发送的登录 Cookie；错误 Key 不能靠 Cookie 回退。不要把示例里的环境变量名误当作 HTTP 请求头。

### 10.1 先取得组织 API Key

拥有组织管理权限的账号可进入相应组织管理页，在“API 密钥”处新建 Key。用途只选读取服务器所需的 `server.view`，服务器范围选目标服务器。密钥在创建成功响应中只显示一次；把它保存到你控制的客户端环境变量或秘密管理系统。Key 列表后续用于查看标签、范围及撤销，不再返回明文令牌。需要轮换时创建新 Key、更新客户端，再撤销旧 Key。

若需要直接调创建 Key 的 API，路由是 `POST /api/orgs/{orgId}/keys`，只能由拥有组织的登录账号调用，写请求同样需 `Content-Type: application/json` 与 `X-Requested-With: warcon`。请求字段包括 `label`、`capabilities`、`serverIds` 和 `expiresDays`；返回 201，包含 `key` 和**仅本次返回**的 `token`。对只读快照的客户端，应将 `capabilities` 设为 `["server.view"]`，并把 `serverIds` 设为目标服务器 ID 数组。不要把这个建 Key 接口嵌入公开客户端。更详细的字段约束以 `src/lib/server/apikeys.ts` 的 `createKey` 校验为准。

下面是可核对的最小创建请求体，`orgId` 从组织管理页 URL 取得；`serverIds` 必须至少包含一个属于该组织的服务器。`expiresDays: 30` 表示 30 天后到期，服务端允许 0～3650，0 表示不设置到期日；实际交付时应按组织政策选定期限。`label` 去掉首尾空白后至少 2 字符、最多保留 60 字符。`serverIds: null` 表示该组织的所有服务器，不适合只读取单台服务器的客户端。

```json
{
  "label": "报表只读客户端",
  "capabilities": ["server.view"],
  "serverIds": ["替换为组织内的服务器编号"],
  "expiresDays": 30
}
```

`GET /api/orgs/{orgId}/keys` 列出该组织 Key 的标签、权限、范围和时间信息，不再返回明文 `token`。`DELETE /api/orgs/{orgId}/keys/{keyId}` 立即撤销对应 Key，成功返回 `{ "ok": true }`。这两个接口同样要求组织所有者的登录会话；DELETE 还要带 `X-Requested-With: warcon`。撤销不删除审计记录。创建、列表和撤销 Key 都是组织管理操作，不接受拿已有 Key 自己管理组织。

### 10.2 推荐的前端调用方法

在仓库的 Svelte 页面中复用 `$lib/api`，它负责 JSON 序列化、同源 Cookie、`X-Requested-With`、`cache: no-store`，并把非 2xx 响应转为包含 `status` 与 `code` 的错误。不要在组件里复制粘贴组织 Key。

```ts
import { api } from '$lib/api';
import type { PersonalPlugin, PluginSnapshot } from '$lib/plugins/sdk';

type ListResponse = { ok: true; plugins: PersonalPlugin[] };
type SnapshotResponse = { ok: true; apiVersion: 1; snapshot: PluginSnapshot | null };

const list = await api<ListResponse>('GET', '/api/plugins');
const current = list.plugins[0];
if (current) {
  const result = await api<SnapshotResponse>(
    'GET', `/api/plugins/${encodeURIComponent(current.manifest.id)}/data`
  );
  if (result.snapshot?.stale) console.warn('玩家快照已过期');
}
```

### 10.3 完整的个人配置 API 表

| 方法与路径 | 请求体 | 成功状态与关键字段 | 失败时重点检查 |
| --- | --- | --- | --- |
| `GET /api/plugins` | 无 | 200 `{ok:true,plugins:[...]}`；按更新时间从新到旧 | 401 未登录 |
| `POST /api/plugins` | `{manifest,serverId,enabled}` | 201 `{ok:true,plugin:{...}}` | 400 清单、409 重复或满额、413 过大、415 类型错误 |
| `GET /api/plugins/{pluginId}` | 无 | 200 `{ok:true,plugin:{...}}` | 404 当前账号找不到 |
| `PUT /api/plugins/{pluginId}` | 与 POST 相同，完整替换 | 200 `{ok:true,plugin:{...}}` | 400 id 不一致、404 原插件不存在 |
| `DELETE /api/plugins/{pluginId}` | 无 | 200 `{ok:true}` | 404 当前账号找不到 |
| `GET /api/plugins/{pluginId}/data` | 无 | 200 `{ok:true,apiVersion:1,snapshot:...}` | 404 找不到、409 已停用、403 或 404 服务器权限变化 |

`plugin` 的结构是 `{manifest, serverId, enabled, updatedAt}`；`updatedAt` 是 UTC ISO 时间字符串。服务端按账号隔离查询：知道别人的 `pluginId` 也不能读取他的个人配置。`GET /api/plugins`、`GET /api/plugins/{id}`、`DELETE /api/plugins/{id}` 不会查询游戏服务器。PUT 是完整替换，不能只发 `{enabled:false}`，也不能改变清单 `id`；先 GET 原 `plugin`，保留 `manifest`、`serverId`，只改 `enabled` 再 PUT。要制作副本，用新 id POST。

创建或更新时，只要 `serverId` 非空，服务端就校验当前账号对该服务器的 `server.view`；之后每次读取数据也会再次校验。因此管理员之后失去授权，旧配置仍可能出现在列表，但不会继续泄露该服务器的快照。`GET /api/plugins/{id}/data` 未关联服务器时返回 `snapshot: null`；这与空玩家列表不是一回事。

### 10.4 可复制的浏览器请求

下面代码要在**已登录的同源面板页面**的开发者工具里运行，或放进面板源码；其他网站的页面不能直接借用面板 Cookie。用实际服务器 ID 替换示例值。不想关联服务器时把 `serverId` 保持 `null`。

```js
const manifest = {
  apiVersion: 1,
  id: 'duty-board',
  name: '值班公告',
  renderer: 'cards',
  widgets: [{ type: 'text', title: '今日值班', text: '先核对玩家与服务器。' }]
};
const response = await fetch('/api/plugins', {
  method: 'POST',
  credentials: 'same-origin',
  cache: 'no-store',
  headers: {
    'Content-Type': 'application/json',
    'X-Requested-With': 'warcon'
  },
  body: JSON.stringify({ manifest, serverId: null, enabled: true })
});
const body = await response.json();
if (!response.ok) throw new Error(`${response.status}: ${body.error?.message}`);
console.log(body.plugin);
```

读取、停用、删除可以沿用同一会话：

```js
const id = encodeURIComponent('duty-board');
const get = await fetch(`/api/plugins/${id}`, { credentials: 'same-origin' });
const { plugin } = await get.json();
const stop = await fetch(`/api/plugins/${id}`, {
  method: 'PUT',
  credentials: 'same-origin',
  headers: { 'Content-Type': 'application/json', 'X-Requested-With': 'warcon' },
  body: JSON.stringify({ manifest: plugin.manifest, serverId: plugin.serverId, enabled: false })
});
console.log(stop.status, await stop.json());
// 需要删除时再调用 DELETE；不要在停用流程中自动删除。
```

### 10.5 命令行请求头示例

独立后台通常只需快照接口。类 Unix Shell 可将 Key 放进当前进程环境变量，然后发请求：

```sh
export WARCON_API_KEY='替换为自己的组织 API Key'
curl --fail-with-body \
  --header "Authorization: Bearer ${WARCON_API_KEY}" \
  --header 'Accept: application/json' \
  'https://你的面板域名/api/servers/服务器编号/plugin-snapshot'
```

Windows PowerShell 示例无需使用 Shell 字符串拼接 URL；把真实值替换进变量：

```powershell
$panelOrigin = 'https://你的面板域名'
$targetServerId = '服务器编号'
$warconApiKey = $env:WARCON_API_KEY
$headers = @{ Authorization = "Bearer $warconApiKey"; Accept = 'application/json' }
$url = "$panelOrigin/api/servers/$([uri]::EscapeDataString($targetServerId))/plugin-snapshot"
$result = Invoke-RestMethod -Method Get -Uri $url -Headers $headers
$result.snapshot.metrics
```

命令行示例只展示读接口。不要用浏览器地址栏访问并试图加 Bearer：地址栏不能设置 `Authorization` 头；也不要把 Key 拼进 `?key=` 查询参数，服务端不按这种方式认证，而且 URL 可能进入日志与历史记录。

## 11. 快照字段、时间和空值的完整解释

`GET /api/servers/{serverId}/plugin-snapshot` 和关联服务器后的个人插件数据接口共享同一个 `PluginSnapshot` 结构。外层 JSON 不同：独立服务器接口为 `{ok:true,snapshot:...}`；个人数据接口多一个外层 `apiVersion:1`，且 `snapshot` 可为 `null`。快照内部也有 `apiVersion:1`。不要只凭外层字段判断数据是否新鲜。

| 字段 | 类型 | 准确解释 |
| --- | --- | --- |
| `server.id` / `server.name` | 字符串 | 经过权限检查的服务器 ID 与名称 |
| `server.map` | 字符串或 `null` | 最近状态快照中的地图名，可能没有采到 |
| `statusAt` | ISO 时间或 `null` | 状态快照的记录时间；不代表玩家列表时间 |
| `playersAt` | ISO 时间或 `null` | 玩家列表快照的记录时间；没有采到则为空 |
| `stale` | 布尔 | `playersAt` 缺失或距离当前时间超过 90 秒即为 true |
| `metrics.online` | 数字或 `null` | 已采到玩家数组时取原数组长度；未采到则为 null |
| `metrics.kills` / `deaths` / `cash` | 数字或 `null` | 当前在线玩家相应字段全都有有效数字，且原数组不超过 256 人时求和，否则为 null |
| `metrics.averagePing` | 数字或 `null` | 当前有有效 ping 的玩家平均值，四舍五入到整数；没有有效值或超过 256 人时为 null |
| `players` | 数组 | 最多取原快照前 256 项；每项只含下文列出的白名单字段 |
| `playersTruncated` | 布尔 | 原玩家数组超过 256 项时为 true；此时合计指标和平均 ping 不可靠，返回 null |

每个 `players` 元素的字段是 `name: string`、`steamId: string`、`faction: string | null`、`kills: number | null`、`deaths: number | null`、`cash: number | null`、`ping: number | null`。玩家名与 SteamID 从保存的玩家快照转换成字符串；阵营只有原值是字符串才返回，否则为 `null`。四项数值只有原值为有限数时才返回，否则为 `null`。没有 `ip`、`region`、RCON 密码、Feed 令牌、Steam Web API Key 或 AI Key 字段。

`stale: false` 只能说明数据库里的玩家快照在 90 秒内更新，不保证击杀事件连续、地图回调已接通，也不证明某个玩家的统计完整。`statusAt` 有值而 `playersAt` 为 `null` 时，地图可能显示出来，但在线人数和合计指标仍不可用。若 `playersAt` 有值且 `players` 为空数组，`online` 会是 **0**，三项合计会是 **0**；这是与“尚未采集”的 `null` 明确不同的状态。绘图时让缺失值断开，不要画成零点。

快照接口只读取数据库 `server_live` 保存的最近状态。调用 API 不触发一次新的 RCON 轮询，不创建击杀、伤害或反作弊案件。某些数据久不更新时要排查 Worker、游戏服务器连接或采集流程，而不是缩短客户端轮询间隔。内置运行页约每 10 秒读取一次，独立客户端也宜按可接受的数据延迟控制频率，并在网络错误、权限错误时明确标注旧结果的时间。

### 11.1 建议的客户端状态处理

```ts
import type { PluginSnapshot } from '$lib/plugins/sdk';

function describe(snapshot: PluginSnapshot | null): string {
  if (!snapshot) return '未关联服务器';
  if (!snapshot.playersAt) return '尚未采集玩家列表';
  if (snapshot.stale) return `玩家数据已过期：${snapshot.playersAt}`;
  if (snapshot.playersTruncated) return '玩家超过 256 人，列表和合计受限';
  const online = snapshot.metrics.online;
  return online === null ? '在线人数暂不可用' : `当前在线 ${online} 人`;
}
```

如果业务必须使用全量玩家、全场历史收入或一局内曲线，不要用 `players` 或 `metrics.cash` 冒充。应先说明数据来源、授权范围、采样时点和延迟，再设计新的后端路由和测试。这个插件 API 的目的只是安全地复用现有在线快照。

## 12. 代码插件：从仓库示例改造成自己的组件

代码插件与 JSON 卡片的安装方式不同。JSON 是数据配置，普通账号可以在 `/plugins` 自己导入；Svelte 组件是面板程序的一部分，需要能修改仓库源码、审查代码、构建并部署的人操作。只导入 `renderer: "my-summary"` 的 JSON，不会安装同名 Svelte 文件。开发公司交付插件时，应同时交付源码、清单、变更说明和验证步骤。

第一步，复制 `src/lib/plugins/extensions/round-summary/` 为新目录，目录名使用稳定英文短名，例如 `my-summary`。在 `manifest.json` 设置唯一 `id`、显示名称、`renderer`；`renderer` 与注册表键保持完全一致。`id` 是用户个人配置的标识，`renderer` 是组件选择键，二者可以相同，便于维护。`apiVersion` 保持 1。代码插件当前不要求 `widgets` 有内容。

第二步，写 `Plugin.svelte`。组件的输入只有 `plugin`、`snapshot`、`loading`、`error` 四个 props。`plugin` 是当前用户保存的清单，可能与仓库内示例清单的名称和样式不同；不要只从本地导入的 `manifest.json` 读设置。`snapshot` 可能是 `null`，其中数值也可能为 `null`。`loading` 表示运行页正在读取，`error` 是读数据错误文本或 `null`。组件不需要自己开定时器，因为运行页负责刷新。

```svelte
<script lang="ts">
  import type { PluginComponentProps } from '$lib/plugins/sdk';
  let { plugin, snapshot, loading, error }: PluginComponentProps = $props();

  let onlineText = $derived(
    snapshot?.metrics.online === null || snapshot?.metrics.online === undefined
      ? '—'
      : String(snapshot.metrics.online)
  );
</script>

<section class="panel my-summary" style:--plugin-accent={plugin.style.accent}>
  <h2 class="text-xl font-semibold text-white">{plugin.name}</h2>
  {#if error}<p class="mt-2 text-warn" role="alert">{error}</p>{/if}
  {#if loading}<p class="mt-2 text-mist-400">正在读取玩家快照…</p>{/if}
  {#if snapshot?.stale}<p class="mt-2 text-warn">玩家数据已过期</p>{/if}
  <p class="mt-3">在线人数：{onlineText}</p>
  <p class="text-sm text-mist-400">{snapshot?.server.name ?? '尚未关联服务器'}</p>
</section>

<style>
  .my-summary { padding: 1.5rem; border-top: 3px solid var(--plugin-accent); }
</style>
```

第三步，在 `catalog.ts` 导入并解析清单，在 `components.ts` 导入组件并按 renderer 注册；第 4.3 节已经给出精简片段。注册后运行 `bun run check`，能发现字段类型或 Svelte 组件 props 不匹配。运行 `bun run build` 才会把组件编译进部署产物。仅把源码文件放到 VPS 而不重新构建，已运行的网站仍找不到组件。

第四步，由有部署权限的人按项目既有流程发布完整构建和数据库迁移。部署前保存原构建、确认当前版本能启动；部署后用普通授权账号进 `/plugins`，在“使用：我的概况”选择代码插件、关联服务器、打开运行页。若页面提示“该代码组件不在当前构建中”，优先检查 `renderer` 的两个注册表及实际部署的构建版本。代码插件渲染出错时，运行页有错误边界及“重试插件”按钮；这只恢复界面，不修复代码或权限。

### 12.1 和面板保持同一种语言与样式

面向管理员的按钮、空状态、错误提示写中文，稳定的数据字段和代码标识用英文。优先复用现有 `panel`、`btn`、`input` 和颜色类；例如标题 `text-white`、次要说明 `text-mist-400`、错误 `text-warn`。不要在组件中引入另一份 Svelte、全局 UI 框架或整站 CSS 重置。组件 `<style>` 默认作用域内生效，自己的类名前缀可以进一步避免冲突；不要写会覆盖全站导航、按钮或表格的 `:global(...)` 规则。

样式变量可用 `--plugin-` 前缀，并仅从经过清单校验的 `plugin.style` 取值。显示玩家名等外部内容时使用 Svelte 文本插值 `{value}`；不要用 `{@html}` 把玩家输入渲染成 HTML。外部图片、脚本、iframe 或 CDN 资源会引入额外隐私和安全问题，开发公司应在代码审查时明确评估，不能通过 JSON 清单绕过审核。

### 12.2 如果确实需要新的后端数据

这属于新增项目功能，不能在客户端凭空拼接数据库地址或直接连接 RCON。设计时先写清楚字段定义、采集来源、更新时间、权限和缺失值语义。服务端路由要通过现有 `route()` 包装返回统一 JSON 错误；涉及服务器数据时使用 `requireServerCap(env, locals, serverId, 'server.view')` 或更严格的实际能力；按最小字段返回，保留 `Cache-Control: no-store`。写接口还要遵守 hooks 的 CSRF 规则及相应更高权限。补上针对无权限、Key 范围之外、空快照及异常输入的测试，再在插件组件中调用。上述是扩展开发原则，不代表当前已经提供了某个自定义路由。

## 13. 错误响应与排查顺序

正常 API 响应带 `ok: true`；应用错误通常为 `{ "ok": false, "error": { "message": "...", "code": "..." } }`。`error.code` 在某些 400、413、415 错误中可能省略，客户端应以 HTTP 状态码和 `message` 一起判断，不能假设每个失败都有固定 code。服务端还可能返回 `extra`，当前插件接口不依赖它。成功响应和由路由包装产生的错误响应都设置 `Cache-Control: no-store`。

| HTTP 状态 | 可能的 code | 常见原因 | 先做什么 |
| --- | --- | --- | --- |
| 400 | `invalid_json` | 请求体不是合法 JSON | 用 JSON 校验器检查引号、逗号、编码 |
| 400 | `invalid_plugin` | 清单字段、类型、长度或额外字段不符合 v1 | 看 `error.message` 中的字段路径 |
| 400 | `unknown_renderer` | 清单指定的代码组件未注册 | 核对两个注册表并重新构建 |
| 400 | 可能无 code | PUT 时清单 id 与 URL id 不一致 | 保持原 id；复制时改用 POST |
| 401 | `unauthenticated` | 会话缺失或过期 | 在面板重新登录 |
| 401 | `invalid_api_key` 等 | Bearer 格式、有效性或期限有问题 | 检查 Key 类型、撤销状态、实际发送的头 |
| 403 | `csrf` | 写请求缺少精确的 `X-Requested-With: warcon` | 在同源客户端加请求头 |
| 403 | `api_key_forbidden` | 用组织 Key 调个人插件或管理接口 | 个人配置改用登录会话 |
| 403 | `forbidden` | 看得到服务器但没有对应能力 | 核对 `server.view` 授权 |
| 403 | `must_change_password` / `enrolment_required` | 账号安全设置未完成 | 去账号页按提示完成 |
| 404 | `not_found` | 当前账号无该插件、服务器不存在或不可见 | 检查 URL id 和账号/组织范围 |
| 409 | `plugin_exists` | 当前账号已有同一插件 id | 编辑旧插件或更换 id |
| 409 | `plugin_limit` | 当前账号已有 20 个个人插件 | 删除不用的配置后再新增 |
| 409 | `plugin_disabled` | 调用了已停用插件的数据接口 | 在配置页启用，或停止轮询 |
| 413 | 可能无 code | POST/PUT 的 JSON 超过 65,536 字节 | 删除冗余文本，压缩清单字段 |
| 415 | 可能无 code | 写请求不是 `application/json` | 设置正确的 Content-Type |
| 500 | `internal` | 服务端内部错误 | 记录时间与请求路径，查服务端日志 |

建议按“网络/域名 → 登录或 Key → 组织及服务器权限 → JSON 格式 → 数据时间 → 组件注册”的顺序排查。浏览器 Network 面板里先看实际方法、URL、状态、Request Headers 和 Response JSON；不要只看页面写的“没有数据”。对 Key 请求要确认 `Authorization` 真正发送到了面板，且不是发送给游戏服 RCON 地址。对会话请求要确认 Cookie 属于当前面板域名，并且不是跨站页面在调用。

### 13.1 五个可复现的诊断例子

1. `POST /api/plugins` 返回 403 `csrf`：通常是自己写的 `fetch` 漏了 `X-Requested-With: warcon`。使用 `$lib/api` 或按第 10.4 节补头。
2. 清单导入页面能过，直接 POST 返回 400：检查是否把清单包进 `manifest`，并提供合法的 `serverId` 或 `null`。
3. 编辑时只发 `{enabled:false}` 返回 400：PUT 要带完整 `manifest`；可先 GET 再 PUT。
4. `/data` 返回 409：配置已停用；这是接口拒绝读取的预期行为，不是游戏服务器故障。
5. API 返回 200，但指标为 `null`：看 `playersAt`、`stale`、`playersTruncated`、玩家字段缺失情况。200 说明请求被接受，不表示数据已完整采到。

## 14. 给开发公司交付和验收的清单

请在交付包中写明插件名称、`id`、`renderer`、`apiVersion`、版本、目标面板代码版本、所需服务器权限、展示字段来源、刷新策略、空值处理、故障提示和回退方法。JSON 卡片交付 `.json` 文件即可，另附导入步骤和截图说明；代码组件需交付可审查的 `.svelte`、`manifest.json`、两个注册表变更及必要测试。若增加 API，还需交付路由源码、请求/响应样例、认证与权限矩阵、迁移及部署说明。

推荐的验收场景：未关联服务器时静态文字可见而指标不伪造；关联有权服务器后能显示最近快照；失去 `server.view` 后不再返回数据；停用后 `/data` 为 409；另一个账号即使知道 id 也不能读取个人配置；一个有范围限制的组织 Key 只能读获准服务器；玩家数据缺失时出现“—”；超过 90 秒时提示过期；代码组件的 `renderer` 在当前构建可找到；手机宽度下表格可滚动且无全局样式污染。

验收时不要把真实 RCON 密码、Steam API Key、AI Key、组织 Key 写入测试清单或 issue。对外展示截图中的玩家名和 SteamID 可能属于社区数据，应按组织管理规则处理。JSON 插件没有执行权限；代码插件在面板构建中运行，需要按普通代码变更审查。删除个人配置不会撤销组织 API Key；不再使用的 Key 要在组织设置里单独撤销。

## 15. 当前实现的限制与版本升级规则

API v1 的 `PluginSnapshot` 只覆盖最近在线快照。它不包含历史场次、伤害事件、击杀事件完整序列、经济曲线、案件审核或自动处罚数据。若要做这些功能，应先确认已有可靠数据源和角色权限，再设计单独的数据协议。通过在清单里增写 `killsHistory`、`caseScore` 等未知字段不会自动得到数据，只会被严格校验拒绝。

以后升级协议时，先在 `sdk.ts` 定义新增字段的类型与默认值，再更新服务端白名单和组件、写兼容测试，最后文档标记新的 `apiVersion`。不能只改文档示例，也不能在不同组件里各自解释同一字段。特别是 `null` 的语义、时间戳、最多 256 人和 `stale` 90 秒门槛，应作为明确的版本契约管理；若改变，需让客户端有机会判断版本并调整展示。

这份文件是 **GitHub 仓库版完整技术手册**。目前发布到面板上的 `/docs/plugins` 是另一个由构建脚本生成的页面；仅编辑本文件不会更新那个网页。需要以本手册作为交付依据时，请分享仓库中的 `docs/personal-plugins.zh-CN.md`，并注明对应提交版本。
