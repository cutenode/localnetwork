<script lang="ts">
	import ClaimFields from '#lib/components/ClaimFields.svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import { formFields } from '#lib/form-fields.svelte.ts';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const values = $derived(form?.values ?? {});

	const fields = $derived(formFields(values));

	const claimed = $derived(data.project.managers.length > 0);
</script>

<svelte:head><title>Claim {data.project.profile.name} · Local Network</title></svelte:head>

<div class="flex max-w-2xl flex-col gap-8">
	<SectionHeader level={1} eyebrow={claimed ? 'Co-manage' : 'Claim'} title={data.project.profile.name} />
	<p class="notice notice-info m-0">
		{#if claimed}This project already has managers. Ask to join them, and a moderator will check the evidence you give.{:else}Show this project is yours and a moderator will hand it over. Once approved you can propose edits and get booking inquiries.{/if}
	</p>
	<form method="POST" class="card flex flex-col gap-5 p-5 sm:p-6">
		<FormMessage {form} />
		<ClaimFields {values} claimant={data.claimant} />
		<div>
			<label class="label" for="message">Anything else? <span class="font-normal text-fg-subtle">(optional)</span></label>
			<textarea id="message" name="message" class="input min-h-24" maxlength="2000" bind:value={fields.message}></textarea>
		</div>
		<div class="flex flex-wrap items-center justify-end gap-3 border-t border-line pt-5">
			<a href="/projects/{data.project.slug}" class="btn-ghost">Cancel</a>
			<button class="btn-primary">Submit claim</button>
		</div>
	</form>
</div>
