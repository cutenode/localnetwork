<script lang="ts">
	import ClaimFields from '#lib/components/ClaimFields.svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import { formFields } from '#lib/form-fields.svelte.ts';
	import { claimTerms } from '#lib/requests.ts';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const values = $derived(form?.values ?? {});

	const fields = $derived(formFields(values));

	const terms = $derived(claimTerms({ space: data.venue.space }));
</script>

<svelte:head><title>Claim {data.venue.name} · Local Network</title></svelte:head>

<div class="flex max-w-2xl flex-col gap-8">
	<SectionHeader level={1} title="Claim {data.venue.name}" lede={data.venue.locality} />
	<p class="notice notice-info">Run this venue? Show it, and a moderator will add you as an owner. Owners confirm events here and add booking managers.</p>
	<form method="POST" class="card flex flex-col gap-5 p-5 sm:p-6">
		<FormMessage {form} />
		<ClaimFields {values} claimant={data.claimant} {terms} />
		<div>
			<label class="label" for="message">Anything else? <span class="font-normal text-fg-subtle">(optional)</span></label>
			<textarea id="message" name="message" class="input min-h-24" maxlength="2000" bind:value={fields.message}></textarea>
		</div>
		<div class="flex flex-wrap items-center justify-end gap-3 border-t border-line pt-5">
			<a href="/dashboard" class="btn-ghost">Cancel</a>
			<button class="btn-primary">Submit claim</button>
		</div>
	</form>
</div>
