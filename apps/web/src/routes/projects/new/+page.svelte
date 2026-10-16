<script lang="ts">
	import FormMessage from '#lib/components/FormMessage.svelte';
	import ProjectForm from '#lib/components/ProjectForm.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
</script>

<svelte:head><title>List a project · Local Network</title></svelte:head>

<div class="container-page py-10 sm:py-14">
	<div class="flex max-w-4xl flex-col gap-8">
		<SectionHeader level={1} eyebrow="New project" title="List a DJ project" lede="Tell promoters what you play and where." />
		<div class="flex flex-col gap-3">
			<p class="notice notice-info m-0">A moderator reviews new projects before they go live. You'll see the status on your dashboard. Already listed? <a class="link" href="/projects">Find it</a> and claim it instead.</p>
			{#if !data.viewer?.spacesCapable}
				<p class="notice notice-info m-0">Your PDS doesn't support atproto spaces yet, so Local Network will file this request for you from its root account.</p>
			{/if}
		</div>
		<FormMessage {form} />
		<ProjectForm values={form?.values ?? {}} showMessage geocoder={data.geocoder} />
	</div>
</div>
