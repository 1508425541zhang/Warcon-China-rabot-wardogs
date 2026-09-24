<script lang="ts">
	import { goto } from '$app/navigation';
	import RegisterForm from '$lib/components/RegisterForm.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
</script>

<svelte:head><title>首次设置 · {data.appName}</title></svelte:head>

<p class="mb-1 text-[13px] leading-relaxed text-mist-400">
	首次运行：创建管理服务器和用户的所有者账号。请妥善保存登录方式；建议先使用通行密钥，再到账号页面添加第二种登录方式。
</p>
<RegisterForm
	registerAction="?/password"
	signInHref={null}
	passkeyLabel="使用通行密钥创建所有者账号"
	registerLabel="创建所有者账号"
	tokenRequired={data.tokenRequired}
	{form}
	onPasskeyDone={() => goto('/', { invalidateAll: true })}
/>
