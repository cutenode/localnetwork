<script lang="ts">
	import { enhance } from '$app/forms';
	import { UserRound } from '@lucide/svelte';

	/**
	 * Asks someone without a profile to make one, started from their Bluesky profile: its name here, and its picture
	 * when `hasAvatar`. Everything else waits for the full editor.
	 */
	let { name, hasAvatar }: { name: string; hasAvatar: boolean } = $props();

	// Bound, so a name typed before the page hydrated stays (see formFields).
	let typed = $derived(name);

	let submitting = $state(false);
</script>

<section aria-labelledby="setup" class="card flex flex-col gap-4 p-5 sm:p-6">
	<div class="flex items-start gap-3">
		<span class="grid size-10 shrink-0 place-items-center rounded-lg bg-raised text-fg-muted"><UserRound class="size-5" aria-hidden="true" /></span>
		<div class="flex min-w-0 flex-col gap-1">
			<h2 id="setup" class="m-0 text-emphasis font-semibold">Set up your profile</h2>
			<p class="m-0 text-small text-fg-muted">Your page on Local Network. Start from your Bluesky profile and add the rest later.</p>
		</div>
	</div>
	<form
		method="POST"
		action="/dashboard/profile?/create"
		class="flex flex-col gap-3"
		use:enhance={() => {
			submitting = true;

			return async ({ update }) => {
				await update();
				submitting = false;
			};
		}}
	>
		<div class="sm:max-w-sm">
			<label class="label" for="setup-name">Name</label>
			<input id="setup-name" class="input" name="name" bind:value={typed} required maxlength="64" />
		</div>
		{#if hasAvatar}
			<label class="flex items-center gap-2.5 text-small text-fg-soft"><input type="checkbox" class="checkbox" name="blueskyAvatar" checked /> Use my Bluesky picture</label>
		{/if}
		<div class="flex flex-wrap gap-2">
			<button class="btn-primary" disabled={submitting}>{submitting ? 'Creating…' : 'Create profile'}</button>
			<a class="btn-ghost" href="/dashboard/profile">Fill in everything</a>
		</div>
	</form>
</section>
