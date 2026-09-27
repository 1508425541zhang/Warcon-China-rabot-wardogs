// Keep the public guide and its downloadable Markdown in sync with the source document.
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
const source = readFileSync('docs/personal-plugins.zh-CN.md', 'utf8');
const escape = (text: string) =>
	text
		.replaceAll('&', '&amp;')
		.replaceAll('<', '&lt;')
		.replaceAll('>', '&gt;')
		.replaceAll('{', '&#123;')
		.replaceAll('}', '&#125;');
const inline = (text: string) =>
	escape(text)
		.replace(/`([^`]+)`/g, '<code>$1</code>')
		.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
const blocks: string[] = [],
	snippets: string[] = [];
let paragraph: string[] = [],
	table: string[] = [],
	code: string[] = [],
	fence = false;
function flush() {
	if (paragraph.length)
		blocks.push(`<p class="mt-3 leading-7 text-mist-300">${inline(paragraph.join(' '))}</p>`);
	paragraph = [];
	if (table.length) {
		const rows = table.filter((line) => !line.split('').every((char) => '| :-'.includes(char)));
		const cells = (line: string, tag: string) =>
			line
				.slice(1, -1)
				.split('|')
				.map((c) => `<${tag}>${inline(c.trim())}</${tag}>`)
				.join('');
		blocks.push(
			`<div class="table-wrap mt-4"><table><thead><tr>${cells(rows[0], 'th')}</tr></thead><tbody>${rows
				.slice(1)
				.map((r) => `<tr>${cells(r, 'td')}</tr>`)
				.join('')}</tbody></table></div>`
		);
	}
	table = [];
}
for (const line of source.split(/\r?\n/)) {
	if (line.startsWith('```')) {
		if (fence) {
			snippets.push(code.join('\n'));
			blocks.push(
				`<pre class="mt-4 overflow-x-auto rounded border border-white/10 bg-ink-950 p-4 text-xs leading-6"><code>{snippets[${snippets.length - 1}]}</code></pre>`
			);
			code = [];
			fence = false;
		} else {
			flush();
			fence = true;
		}
	} else if (fence) code.push(line);
	else if (line.startsWith('#')) {
		flush();
		const level = Math.min(3, line.match(/^#+/)![0].length);
		blocks.push(
			`<h${level} class="mt-8 text-xl font-semibold text-white">${inline(line.replace(/^#+ /, ''))}</h${level}>`
		);
	} else if (line.startsWith('|')) {
		if (paragraph.length) flush();
		table.push(line);
	} else if (/^(\d+\. |- )/.test(line)) {
		flush();
		blocks.push(`<p class="mt-3 leading-7 text-mist-300">${inline(line)}</p>`);
	} else if (!line.trim()) flush();
	else paragraph.push(line);
}
flush();
mkdirSync('src/routes/docs/plugins', { recursive: true });
mkdirSync('static/docs', { recursive: true });
writeFileSync('static/docs/personal-plugins.zh-CN.md', source);
writeFileSync('src/lib/plugins/guide-snippets.json', JSON.stringify(snippets, null, 2) + '\n');
writeFileSync(
	'src/routes/docs/plugins/+page.svelte',
	`<script lang="ts">import snippets from '$lib/plugins/guide-snippets.json';</script>
<svelte:head><title>个人插件开发与 API · Warcon China</title></svelte:head>
<main class="mx-auto max-w-5xl px-5 py-10">
<div class="flex flex-wrap gap-3"><a class="btn" href="/plugins">← 个人插件</a><a class="btn" href="/examples/plugins/server-cards.json" download>下载 JSON 示例</a><a class="btn" href="/examples/plugins/round-summary.zip" download>下载代码示例</a><a class="btn" href="/docs/personal-plugins.zh-CN.md" download>下载完整文档</a></div>
${blocks.join('\n')}
</main>\n`
);
console.log('Plugin guide generated.');
