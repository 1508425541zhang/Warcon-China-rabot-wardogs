<script lang="ts">
	import { goto } from '$app/navigation';
	import RegisterForm from '$lib/components/RegisterForm.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
</script>

<svelte:head><title>Setup · {data.appName}</title></svelte:head>

<p class="mb-1 text-[13px] leading-relaxed text-mist-400">
	First run. Create the owner account that will manage servers and users. Nobody can reset the
	owner, so start with a passkey, then add a second way in from the account page.
</p>
<RegisterForm
	registerAction="?/password"
	signInHref={null}
	passkeyLabel="Create owner account with a passkey"
	registerLabel="Create owner account"
	tokenRequired={data.tokenRequired}
	{form}
	onPasskeyDone={() => goto('/', { invalidateAll: true })}
/>
