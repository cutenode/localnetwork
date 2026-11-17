<script lang="ts">
	import { enhance } from '$app/forms';
	import type { OnboardingData } from '#lib/onboarding/checklist.ts';

	/**
	 * Makes someone's first profile from a name, started from their Bluesky profile: its name here, and its picture
	 * when it has one. Everything else waits for the full editor.
	 */
	let { data }: { data: OnboardingData } = $props();

	// Bound, so a name typed before the page hydrated stays (see formFields).
	let typed = $derived(data.prefill?.name ?? '');

	let submitting = $state(false);
</script>

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
	{#if data.prefill?.hasAvatar}
		<label class="flex items-center gap-2.5 text-small text-fg-soft"><input type="checkbox" class="checkbox" name="blueskyAvatar" checked /> Use my Bluesky picture</label>
	{/if}
	<button class="btn-primary self-start" disabled={submitting}>{submitting ? 'Creating…' : 'Create profile'}</button>
</form>
