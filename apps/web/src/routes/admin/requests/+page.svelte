<script lang="ts">
	import EmptyState from '#lib/components/EmptyState.svelte';
	import Handle from '#lib/components/Handle.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { REQUEST_KIND_LABEL, requestSubject, timeAgo } from '#lib/format.ts';
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
	<SectionHeader level={1} title="Requests" lede="New projects, claims and changes. You can't review your own requests." />

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

	{#if data.requests.length === 0}
		<EmptyState title="Nothing here" body="No requests match this filter." />
	{:else}
		<div class="card overflow-x-auto">
			<table class="table-base min-w-150">
				<thead><tr><th>Request</th><th>From</th><th>Submitted</th><th>Status</th></tr></thead>
				<tbody>
					{#each data.requests as request (request.uri)}
						<tr class="transition-colors hover:bg-raised">
							<td><a class="font-medium text-fg no-underline hover:underline" href="/admin/requests/{request.rkey}">{REQUEST_KIND_LABEL[request.kind]}: {requestSubject(request)}</a>{#if request.record.onBehalfOf}<p class="text-label text-fg-subtle">Filed by root on their behalf</p>{/if}</td>
							<td><Handle account={request.requester} /></td>
							<td class="whitespace-nowrap text-fg-muted">{timeAgo(request.createdAt)}</td>
							<td><StatusPill status={request.status} /></td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	{/if}
</div>
