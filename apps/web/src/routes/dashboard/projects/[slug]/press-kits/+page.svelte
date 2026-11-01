<script lang="ts">
	import { Plus } from '@lucide/svelte';
	import EpkKitList from '#lib/components/epk/EpkKitList.svelte';
	import EpkViewList from '#lib/components/epk/EpkViewList.svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const project = $derived(data.project);
</script>

<svelte:head><title>Press kits · {project.profile.name}</title></svelte:head>

<div class="flex max-w-208 flex-col gap-6">
	{#snippet add()}<a class="btn-primary btn-sm" href="/dashboard/projects/{project.slug}/press-kits/new"><Plus class="size-4" />New press kit</a>{/snippet}
	<SectionHeader level={1} eyebrow={project.profile.name} title="Press kits" lede="One for each kind of booking. Every manager can edit them through a draft." actions={add} />
	{#if data.deleted}<p role="status" class="notice notice-ok m-0">Press kit deleted.</p>{/if}
	<EpkKitList dashboard={data.dashboard} slug={project.slug} viewerDid={data.viewerDid} />
	<FormMessage {form} />
	<EpkViewList views={data.views} slug={project.slug} retentionDays={data.retentionDays} />
</div>
