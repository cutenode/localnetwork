<script lang="ts">
	import EmptyState from '#lib/components/EmptyState.svelte';
	import Handle from '#lib/components/Handle.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { timeAgo } from '#lib/format.ts';
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

	const kindLabel = { create: 'New project', claim: 'Claim', update: 'Changes' } as const;
</script>

<svelte:head><title>Requests · Staff</title></svelte:head>

<h1 class="text-3xl">Requests</h1>
<p class="muted mt-1">New projects, claims, and changes. You can't review your own requests.</p>
<div class="mt-6 flex flex-wrap gap-2">
	{#each tabs as tab (tab.value)}
		<a href="?status={tab.value}{data.kind ? `&kind=${data.kind}` : ''}" class="btn-sm {data.status === tab.value ? 'btn-primary' : 'btn-secondary'}">
			{tab.label}{#if tab.value !== 'all'} <span class="opacity-70">{data.counts[tab.value]}</span>{/if}
		</a>
	{/each}
	<span class="mx-2 border-l-2 border-ansi-dark"></span>
	{#each [['', 'Any kind'], ['create', 'New'], ['claim', 'Claims'], ['update', 'Changes']] as [value, label] (value)}
		<a href="?status={data.status}{value ? `&kind=${value}` : ''}" class="btn-sm {data.kind === value ? 'btn-primary' : 'btn-ghost'}">{label}</a>
	{/each}
</div>

<div class="mt-6">
	{#if data.requests.length === 0}
		<EmptyState title="Nothing here" body="No requests match this filter." />
	{:else}
		<div class="card overflow-x-auto">
			<table class="table-base">
				<thead><tr><th>Request</th><th>From</th><th>Submitted</th><th>Status</th></tr></thead>
				<tbody>
					{#each data.requests as request (request.uri)}
						<tr class="hover:bg-ansi-blue">
							<td><a class="menu-link" href="/admin/requests/{request.rkey}">{kindLabel[request.kind]}: {request.project?.name ?? request.record.profile?.name ?? '—'}</a>{#if request.record.onBehalfOf}<p class="muted text-xs">filed by root on their behalf</p>{/if}</td>
							<td><Handle account={request.requester} /></td>
							<td class="whitespace-nowrap">{timeAgo(request.createdAt)}</td>
							<td><StatusPill status={request.status} /></td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	{/if}
</div>
