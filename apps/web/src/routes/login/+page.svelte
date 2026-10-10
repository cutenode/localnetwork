<script lang="ts">
	import { AtSign, KeyRound, LockKeyhole } from '@lucide/svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	// The OAuth callback sends `?error=denied` when the user declines and `?error=login` for any other failure.
	const callbackError = $derived(data.error === 'denied' ? 'You declined the request, so we could not sign you in.' : 'Sign-in was cancelled or failed. Try again.');
</script>

<svelte:head><title>Sign in · Local Network</title></svelte:head>

<div class="container-page grid gap-12 py-16 lg:grid-cols-2 lg:py-24">
	<div class="space-y-6">
		<p class="eyebrow">Sign in</p>
		<h1 class="text-4xl sm:text-5xl">Use your Atmosphere account</h1>
		<p class="muted text-lg">Local Network signs you in with the AT Protocol, the same account you use for Bluesky and other atproto apps. We never see your password.</p>
		<ul class="space-y-4 text-sm">
			<li class="flex gap-3"><KeyRound class="size-5 shrink-0 text-ansi-bright-magenta" /><span>You approve exactly what we can do on your PDS: write your own requests, bookings, and audit entries into Local Network's spaces, and keep a private planner.</span></li>
			<li class="flex gap-3"><LockKeyhole class="size-5 shrink-0 text-ansi-bright-magenta" /><span>Private data uses <a class="link" href="/about#spaces">atproto spaces</a>. If your PDS doesn't support spaces yet, you can still sign in and we'll file things on your behalf.</span></li>
		</ul>
	</div>
	<div class="card card-pad self-start">
		<form method="POST" class="space-y-4">
			<FormMessage form={form ?? (data.error ? { error: callbackError } : null)} />
			{#if data.reauth && !form && !data.error}
				<p class="notice notice-info">Local Network needs new permissions. Sign in again to approve them.</p>
			{/if}
			<input type="hidden" name="next" value={data.next} />
			<div>
				<label class="label" for="handle">Handle</label>
				<div class="relative">
					<AtSign class="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-ansi-cyan" />
					<input id="handle" name="handle" class="input pl-9" placeholder="alice.bsky.social" autocomplete="username" autocapitalize="none" spellcheck="false" required value={form?.handle ?? ''} />
				</div>
			</div>
			<button class="btn-accent w-full">Continue</button>
			<p class="hint text-center">You'll be sent to your PDS to approve, then come right back.</p>
		</form>
	</div>
</div>
