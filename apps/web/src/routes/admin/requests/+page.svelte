<script lang="ts">
	import FormMessage from '#lib/components/FormMessage.svelte';
	import RequestTable from '#lib/components/RequestTable.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const tabs = [
		{ value: 'pending', label: 'Pending' },
		{ value: 'changes_requested', label: 'Changes requested' },
		{ value: 'approved', label: 'Approved' },
		{ value: 'rejected', label: 'Rejected' },
		{ value: 'withdrawn', label: 'Withdrawn' },
		{ value: 'all', label: 'All' }
	] as const;

	const kinds = [
		{ value: '', label: 'Any kind' },
		{ value: 'create', label: 'New' },
		{ value: 'claim', label: 'Claims' },
		{ value: 'update', label: 'Changes' },
		{ value: 'list', label: 'Listings' }
	] as const;

	const chip = (active: boolean) => ['chip min-h-9 px-3 transition-colors hover:border-fg-subtle', active && 'border-fg bg-fg font-semibold text-fg-inverse hover:border-fg'];
</script>

<svelte:head><title>Requests · Staff</title></svelte:head>

<div class="flex flex-col gap-6">
	<SectionHeader level={1} title="Requests" lede="New projects, claims, changes and listings. You can't review your own requests." />
	{#if data.decided}<FormMessage form={{ success: `${data.decided}. Nothing else is waiting for you to review.` }} />{/if}

	<div class="flex flex-col gap-3">
		<nav aria-label="Status" class="flex flex-wrap gap-2">
			{#each tabs as tab (tab.value)}
				{@const active = data.status === tab.value}
				<a href="?status={tab.value}{data.kind ? `&kind=${data.kind}` : ''}" aria-current={active ? 'page' : undefined} class={chip(active)}>
					{tab.label}{#if tab.value !== 'all'}<span class="font-mono {active ? '' : 'text-fg-subtle'}">{data.counts[tab.value]}</span>{/if}
				</a>
			{/each}
		</nav>
		<nav aria-label="Kind" class="flex flex-wrap gap-2">
			{#each kinds as kind (kind.value)}
				{@const active = data.kind === kind.value}
				<a href="?status={data.status}{kind.value ? `&kind=${kind.value}` : ''}" aria-current={active ? 'page' : undefined} class={chip(active)}>{kind.label}</a>
			{/each}
		</nav>
	</div>

	<RequestTable requests={data.requests} />
</div>
