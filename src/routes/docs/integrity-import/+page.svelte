<svelte:head>
	<title>外服历史数据导入说明 · Warcon China</title>
	<meta name="description" content="WARDOGS 社区风控外服历史数据 JSON 和 JSONL 导入说明" />
</svelte:head>

<main class="mx-auto max-w-4xl px-5 py-10 sm:px-8 sm:py-14">
	<div class="mb-8 flex flex-wrap items-center justify-between gap-3">
		<a href="/" class="text-sm text-accent hover:text-accent-2">← 返回管理面板</a>
		<span class="rounded border border-accent/40 px-2.5 py-1 text-xs text-accent">WARDOGS · 社区风控</span>
	</div>
	<header class="mb-10 border-b border-white/10 pb-8">
		<p class="mb-3 text-xs font-semibold tracking-[0.18em] text-accent">数据导入指南</p>
		<h1 class="text-3xl font-semibold text-white sm:text-4xl">外服历史数据导入</h1>
		<p class="mt-4 max-w-3xl text-base leading-7 text-mist-300">
			组织 Owner 可以上传其他服务器的逐次击杀记录，审核后用于实验性统计。导入记录不会写入本服实时战绩、排行榜或玩家处罚记录。
		</p>
	</header>

	<nav class="mb-10 flex flex-wrap gap-2 text-sm" aria-label="页面目录">
		<a class="rounded border border-white/10 px-3 py-2 hover:border-accent" href="#prepare">1 整理数据</a>
		<a class="rounded border border-white/10 px-3 py-2 hover:border-accent" href="#schema">2 填写字段</a>
		<a class="rounded border border-white/10 px-3 py-2 hover:border-accent" href="#review">3 上传审核</a>
		<a class="rounded border border-white/10 px-3 py-2 hover:border-accent" href="#result">4 查看结果</a>
	</nav>

	<section id="prepare" class="mb-8 rounded border border-white/10 bg-ink-900 p-5 sm:p-7">
		<h2 class="text-xl font-semibold text-white">1. 整理真实击杀记录</h2>
		<p class="mt-4 leading-7 text-mist-100">
			每条记录代表一次真实的步兵击杀，必须有稳定且唯一的 <code>eventId</code>。只接收最近 30 天、至少 10 分钟前发生的事件。不要用 K/D 汇总、演示值或推测值代替逐次事件。
		</p>
		<div class="mt-5 grid gap-3 sm:grid-cols-2">
			<div class="rounded bg-ink-800 p-4"><strong class="text-white">.jsonl（推荐）</strong><p class="mt-1 text-sm leading-6 text-mist-300">每行一个 JSON 对象。方便分批导出，也容易定位出错行。</p></div>
			<div class="rounded bg-ink-800 p-4"><strong class="text-white">.json</strong><p class="mt-1 text-sm leading-6 text-mist-300">JSON 数组，或者包含 <code>events</code> 数组的对象。</p></div>
		</div>
		<p class="mt-4 text-sm text-mist-300">使用 UTF-8 编码。单个文件最多 8 MiB、10,000 条；更大的导出请按事件拆分批次。</p>
	</section>

	<section id="schema" class="mb-8 rounded border border-white/10 bg-ink-900 p-5 sm:p-7">
		<h2 class="text-xl font-semibold text-white">2. 按标准字段转换</h2>
		<p class="mt-4 leading-7 text-mist-100">下面是一条记录的格式示例。请换成真实的 UTC 时间、Steam ID 和局编号；示例时间过期后不能直接上传。</p>
		<pre class="mt-4 overflow-x-auto rounded border border-white/10 bg-ink-950 p-4 text-xs leading-6 text-mist-100 sm:text-sm"><code>{`{
  "eventId": "kill-000001",
  "eventAt": "2026-09-24T12:00:00.000Z",
  "instanceId": "server-start-001",
  "matchId": "round-001",
  "eventTime": 421.5,
  "map": "Kavkazi",
  "killerSteamId": "76561198000000888",
  "victimSteamId": "76561198000000999",
  "killerFaction": "Blue",
  "victimFaction": "Red",
  "cause": "Id.Item.AK74M",
  "distanceM": 85.4,
  "headshot": false,
  "penetration": false,
  "playerCount": 42
}`}</code></pre>
		<div class="mt-5 overflow-x-auto">
			<table class="w-full min-w-[590px] text-left text-sm leading-6">
				<thead><tr class="border-b border-white/20 text-white"><th class="w-44 px-3 py-2">字段</th><th class="px-3 py-2">填写规则</th></tr></thead>
				<tbody>
					<tr><td><code>eventId</code></td><td>原服务器中稳定唯一的事件 ID；重导时不能换 ID。</td></tr>
					<tr><td><code>eventAt</code></td><td>绝对 UTC 时间，ISO 8601 格式，以 <code>Z</code> 结尾。</td></tr>
					<tr><td><code>instanceId</code></td><td>原服务器本次启动的标识。</td></tr>
					<tr><td><code>matchId</code></td><td>本局唯一编号；同一局保持相同。</td></tr>
					<tr><td><code>eventTime</code></td><td>本局开始后经过的秒数，不是 Unix 时间戳。</td></tr>
					<tr><td><code>map</code></td><td>游戏地图代码；同一地图名称须一致。</td></tr>
					<tr><td><code>killerSteamId</code><br /><code>victimSteamId</code></td><td>各 17 位 Steam ID，且不能相同。</td></tr>
					<tr><td><code>killerFaction</code><br /><code>victimFaction</code></td><td>击杀时的真实阵营，必须不同。</td></tr>
					<tr><td><code>cause</code></td><td>原始武器代码，例如 <code>Id.Item.AK74M</code>。未知枪械先在武器映射里审核。</td></tr>
					<tr><td><code>distanceM</code></td><td>单位为米；未知填 <code>null</code>。</td></tr>
					<tr><td><code>headshot</code><br /><code>penetration</code></td><td>真实布尔值 <code>true</code> 或 <code>false</code>，不能填字符串。</td></tr>
					<tr><td><code>playerCount</code></td><td>当时服务器人数，1～200；未知填 <code>null</code>。</td></tr>
				</tbody>
			</table>
		</div>
		<p class="mt-5 rounded border-l-2 border-accent bg-accent/5 p-4 text-sm leading-6 text-mist-100">如果原始距离单位是厘米，先除以 100 再填入 <code>distanceM</code>。无法确认单位时填 <code>null</code>；不能猜测爆头、穿透、阵营或人数。缺少必需字段的事件应排除，并记录排除数量和原因。</p>
	</section>

	<section id="review" class="mb-8 rounded border border-white/10 bg-ink-900 p-5 sm:p-7">
		<h2 class="text-xl font-semibold text-white">3. 上传并交审</h2>
		<ol class="mt-4 list-decimal space-y-3 pl-5 leading-7 text-mist-100">
			<li>登录管理面板，以组织 Owner 身份打开“组织社区风控设置”。</li>
			<li>填写固定的来源服务器标识，例如 <code>community-eu-01</code>。同一原服务器以后继续使用它。</li>
			<li>选择 <code>.json</code> 或 <code>.jsonl</code> 文件，点击“上传并暂存”。</li>
			<li>核对记录数、时间范围、地图、武器和已知人数，并检查导出来源。</li>
			<li>确认后点击“审核批准”。误批可点击“撤销批准”。</li>
		</ol>
		<p class="mt-4 text-sm leading-6 text-mist-300">整个文件一次校验：有一条不合格就不导入。系统保存文件 SHA-256、来源、操作人、审核时间和批次状态；重复文件或同一来源重复的 <code>eventId</code> 会被拒绝。</p>
	</section>

	<section id="result" class="mb-10 rounded border border-white/10 bg-ink-900 p-5 sm:p-7">
		<h2 class="text-xl font-semibold text-white">4. 查看统计结果</h2>
		<p class="mt-4 leading-7 text-mist-100">本服与外服基线分开计算。优先使用样本足够的本服基线；本服不足时才使用已批准的外服基线。图表会明确标注“审核通过的外服历史”。</p>
		<ul class="mt-4 list-disc space-y-2 pl-5 leading-7 text-mist-100">
			<li>本服至少需要 200 个可比的 180 秒窗口；外服至少需要 30 个，才能进入实验性统计观察。</li>
			<li>上传 30 条击杀不等于 30 个窗口。同枪械爆头率要求窗口内至少 10 次击杀；最远击杀要求至少 3 次。</li>
			<li>自动处置仍要求指标至少 5,000 个可比窗口，并受在线人数、独立证据、开关和处置上限控制。</li>
		</ul>
		<p class="mt-4 text-sm leading-6 text-mist-300">审批表示允许数据作为实验性参考，不等于证明记录真实。管理员应核查外部数据质量；撤销批准后系统会重新计算基线。</p>
	</section>
	<footer class="border-t border-white/10 pt-6 text-sm text-mist-400">Warcon China · WARDOGS 社区服务器</footer>
</main>

<style>
	td { border-bottom: 1px solid rgb(255 255 255 / 0.08); padding: 0.75rem; vertical-align: top; }
	code { color: var(--color-accent-2); font-family: var(--font-mono); font-size: 0.9em; }
	pre code { color: var(--color-mist-100); }
</style>
