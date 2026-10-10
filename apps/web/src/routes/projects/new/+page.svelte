<script lang="ts">
	import FormMessage from '#lib/components/FormMessage.svelte';
	import ProjectForm from '#lib/components/ProjectForm.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
</script>

<svelte:head><title>List a project · Local Network</title></svelte:head>

<div class="container-page max-w-4xl py-10">
	<p class="eyebrow">New project</p>
	<h1 class="mt-1 text-4xl">List a DJ project</h1>
	<p class="muted mt-2 max-w-2xl">Tell promoters what you play and where. A moderator reviews new projects before they go live; you'll see the status on your dashboard. Is your project already listed? <a class="link" href="/projects">Find it</a> and claim it instead.</p>
	{#if !data.viewer?.spacesCapable}
		<p class="mt-4 notice notice-info">Your PDS doesn't support atproto spaces yet, so Local Network will file this request on your behalf from its root account.</p>
	{/if}
	<div class="mt-8 space-y-6">
		<FormMessage {form} />
		<ProjectForm values={form?.values ?? {}} showMessage geocoder={data.geocoder} />
	</div>
</div>
