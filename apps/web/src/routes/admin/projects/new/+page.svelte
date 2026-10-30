<script lang="ts">
	import { ArrowLeft } from '@lucide/svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import ProjectForm from '#lib/components/ProjectForm.svelte';
	import { draftKey } from '#lib/form-draft.svelte.ts';
	import Card from '#lib/ui/Card.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const values = $derived(form?.values ?? {});
</script>

<svelte:head><title>Add a project · Staff</title></svelte:head>

<div class="flex flex-col gap-6">
	<a href="/admin/projects" class="inline-flex items-center gap-1.5 self-start text-small text-fg-muted no-underline hover:text-fg"><ArrowLeft class="size-4" />Projects</a>
	<SectionHeader level={1} title="Add a project" lede="Admins publish directly to the root account's repo, and the action is audited. Leave managers empty so the DJ can claim it later." />
	<FormMessage {form} />
	<ProjectForm {values} submitLabel="Publish project" geocoder={data.geocoder} draft={draftKey(data.viewer, 'admin:project:new')}>
		{#snippet children(shown)}
			<Card as="section">
				<label class="label" for="managers">Managers <span class="font-normal text-fg-subtle">(optional)</span></label>
				<input id="managers" name="managers" class="input" value={String(shown.managers ?? '')} placeholder="alice.bsky.social, did:plc:…" />
				<p class="hint">Handles or DIDs, separated by commas.</p>
			</Card>
		{/snippet}
	</ProjectForm>
</div>
