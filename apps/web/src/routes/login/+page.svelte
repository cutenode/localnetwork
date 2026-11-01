<script lang="ts">
	import { AtSign, KeyRound, LockKeyhole } from '@lucide/svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import Eyebrow from '#lib/ui/Eyebrow.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	// The OAuth callback sends `?error=denied` when the user declines and `?error=login` for any other failure.
	// Bound, so a handle typed before the page hydrated stays (see formFields).
	let handle = $derived(form?.handle ?? '');

	const callbackError = $derived(data.error === 'denied' ? 'You declined the request, so we could not sign you in.' : 'Sign-in was cancelled or failed. Try again.');
</script>

<svelte:head><title>Sign in · Local Network</title></svelte:head>

<div class="container-page py-12 sm:py-24">
	<div class="flex flex-col items-center gap-8">
		<header class="flex max-w-2xl flex-col items-center gap-3 text-center">
			<Eyebrow>Sign in</Eyebrow>
			<h1 class="m-0 text-title font-semibold">Use your Atmosphere account</h1>
			<p class="m-0 text-fg-muted">The same account you use for Bluesky and other atproto apps. We never see your password.</p>
		</header>
		<form method="POST" class="card flex w-full max-w-md flex-col gap-4 p-5 sm:p-6">
			<FormMessage form={form ?? (data.error ? { error: callbackError } : null)} />
			{#if data.reauth && !form && !data.error}
				<p class="notice notice-info m-0">Local Network needs new permissions. Sign in again to approve them.</p>
			{/if}
			<input type="hidden" name="next" value={data.next} />
			<div>
				<label class="label" for="handle">Handle</label>
				<div class="relative">
					<AtSign class="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-fg-subtle" />
					<input id="handle" name="handle" class="input pl-9" placeholder="alice.bsky.social" autocomplete="username" autocapitalize="none" spellcheck="false" required bind:value={handle} />
				</div>
			</div>
			<button class="btn-primary w-full">Continue</button>
			<p class="hint m-0 text-center">You'll be sent to your PDS to approve, then come right back.</p>
		</form>
		<ul class="m-0 flex w-full max-w-md list-none flex-col gap-4 p-0 text-small text-fg-muted">
			<li class="flex gap-3"><KeyRound class="mt-0.5 size-4 shrink-0 text-fg-subtle" /><span>You approve exactly what we can do on your PDS: write your own requests, bookings and audit entries into Local Network's spaces, and keep a private planner.</span></li>
			<li class="flex gap-3"><LockKeyhole class="mt-0.5 size-4 shrink-0 text-fg-subtle" /><span>Private data uses <a class="link" href="/about#spaces">atproto spaces</a>. If your PDS doesn't support spaces yet, you can still sign in and we'll file things for you.</span></li>
		</ul>
	</div>
</div>
