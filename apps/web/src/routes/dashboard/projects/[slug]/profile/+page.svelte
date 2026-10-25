<script lang="ts">
	import FormMessage from '#lib/components/FormMessage.svelte';
	import ProjectForm from '#lib/components/ProjectForm.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
</script>

<svelte:head><title>Edit {data.project.profile.name} · Local Network</title></svelte:head>

<div class="flex max-w-4xl flex-col gap-8">
	<SectionHeader level={1} eyebrow="Propose changes" title={data.project.profile.name} />
	<p class="notice notice-info m-0">Changes go to a moderator before they're published. You can keep editing the request until it's reviewed.</p>
	<FormMessage {form} />
	<ProjectForm values={form?.values ?? data.values} sets={data.sets} avatarUrl={data.project.avatarUrl} showMessage submitLabel="Send for review" geocoder={data.geocoder}>
		{#snippet footer()}<a href="/dashboard/projects/{data.project.slug}" class="btn-ghost">Cancel</a>{/snippet}
	</ProjectForm>
</div>
