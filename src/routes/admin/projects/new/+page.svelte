<script lang="ts">
	import FormMessage from '#lib/components/FormMessage.svelte';
	import ProjectForm from '#lib/components/ProjectForm.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const values = $derived(form?.values ?? {});
</script>

<svelte:head><title>Add a project · Staff</title></svelte:head>

<h1 class="text-3xl">Add a project</h1>
<p class="muted mt-1 max-w-2xl">Admins can publish directly. The record is written to the root account's repo and the action is audited. Leave managers empty to let the DJ claim it later.</p>
<div class="mt-6 space-y-6">
	<FormMessage {form} />
	<ProjectForm {values} submitLabel="Publish project" geocoder={data.geocoder}>
		<section class="card card-pad space-y-2">
			<label class="label" for="managers">Managers <span class="muted">(optional)</span></label>
			<input id="managers" name="managers" class="input" value={String(values.managers ?? '')} placeholder="alice.bsky.social, did:plc:…" />
			<p class="hint">Handles or DIDs, comma separated.</p>
		</section>
	</ProjectForm>
</div>
