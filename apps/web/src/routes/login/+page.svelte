<script lang="ts">
	import FormMessage from '#lib/components/FormMessage.svelte';
	import HandleInput from '#lib/components/HandleInput.svelte';
	import HowSignInWorks from '#lib/components/HowSignInWorks.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	// The OAuth callback sends `?error=denied` when the user declines and `?error=login` for any other failure.
	const callbackError = $derived(data.error === 'denied' ? 'You declined the request, so we could not sign you in.' : 'Sign-in was cancelled or failed. Try again.');

	const withNext = $derived(data.next === '/dashboard' ? '' : `?next=${encodeURIComponent(data.next)}`);
</script>

<svelte:head>
	<title>Sign in · Local Network</title>
	<meta name="description" content="Sign in to Local Network with your Atmosphere account, the one you use for Bluesky." />
</svelte:head>

<form method="POST" class="card flex flex-col gap-5 p-5 sm:p-7">
	<div class="flex flex-col gap-2">
		<h1 class="m-0 text-heading font-semibold">Hi there.</h1>
		<p class="m-0 text-fg-muted">
			Sign in with your <a class="link" href="https://atmosphereaccount.com" rel="noopener noreferrer" target="_blank">Atmosphere account</a> to {data.purpose ?? 'book DJs and manage your projects'}.
		</p>
	</div>
	{#if data.error && !form}<FormMessage form={{ error: callbackError }} />{/if}
	{#if data.reauth && !form && !data.error}
		<p class="notice notice-info m-0">Local Network needs new permissions. Sign in again to approve them.</p>
	{/if}
	<input type="hidden" name="next" value={data.next} />
	<div class="flex flex-col gap-1.5">
		<HandleInput value={form?.handle ?? ''} invalid={!!form?.error} describedby={form?.error ? 'handle-error' : undefined} />
		{#if form?.error}<p id="handle-error" class="m-0 text-label text-critical">{form.error}</p>{/if}
	</div>
	<button class="btn-primary w-full">Sign in</button>
	<div class="flex flex-wrap items-center justify-between gap-x-4 gap-y-2 text-small">
		<a class="link" href="/login/create{withNext}">Make an account</a>
		<button type="button" class="menu-link cursor-pointer" commandfor="how-sign-in" command="show-modal">How signing in works</button>
	</div>
</form>
<HowSignInWorks id="how-sign-in" />
