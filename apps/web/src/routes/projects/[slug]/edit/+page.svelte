<script lang="ts">
	import FormMessage from '#lib/components/FormMessage.svelte';
	import ProjectForm from '#lib/components/ProjectForm.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
</script>

<svelte:head><title>Edit {data.project.profile.name} · Local Network</title></svelte:head>

<div class="container-page max-w-4xl py-10">
	<p class="eyebrow">Propose changes</p>
	<h1 class="mt-1 text-4xl">{data.project.profile.name}</h1>
	<p class="muted mt-2 max-w-2xl">Changes go to a moderator before they're published. You can keep editing the request until it's reviewed.</p>
	<div class="mt-8 space-y-6">
		<FormMessage {form} />
		<ProjectForm values={form?.values ?? data.values} sets={data.sets} avatarUrl={data.project.avatarUrl} showMessage submitLabel="Send for review" geocoder={data.geocoder}>
			{#snippet footer()}<a href="/projects/{data.project.slug}" class="btn-ghost">Cancel</a>{/snippet}
		</ProjectForm>
	</div>
</div>
