<script lang="ts">
	import { enhance } from '$app/forms';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import VerificationTable from '#lib/components/VerificationTable.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	// Bound, so a handle typed before the page hydrated stays (see formFields).
	let subject = $derived(data.subject);
</script>

<svelte:head><title>Verifications · Local Network</title></svelte:head>

<div class="flex flex-col gap-6">
	<SectionHeader level={1} title="Verifications" lede="Bluesky verifications from the root account. Apps that trust it show a badge while the account's handle and display name stay the same." />
	<FormMessage {form} />
	<div class="grid grid-cols-[minmax(0,1fr)] items-start gap-6 xl:grid-cols-[minmax(0,1fr)_20rem]">
		<VerificationTable verifications={data.verifications} live={data.live} />
		<form method="POST" action="?/grant" class="card flex flex-col gap-4 p-5" use:enhance>
			<h2 class="text-emphasis font-semibold">Verify an account</h2>
			<div>
				<label class="label" for="subject">Handle or DID</label>
				<input id="subject" name="subject" class="input" required placeholder="alice.bsky.social" bind:value={subject} />
				<p class="hint">For project managers. Uses their handle and display name as they are now. A rename turns the badge off until you re-issue it.</p>
			</div>
			<button class="btn-primary w-full">Verify</button>
		</form>
	</div>
</div>
