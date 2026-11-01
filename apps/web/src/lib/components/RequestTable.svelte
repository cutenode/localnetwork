<script lang="ts">
	import EmptyState from '#lib/components/EmptyState.svelte';
	import Handle from '#lib/components/Handle.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { REQUEST_KIND_LABEL, requestSubject, timeAgo } from '#lib/format.ts';
	import type { RequestView } from '#lib/types.ts';
	import ResponsiveTable from '#lib/ui/ResponsiveTable.svelte';

	/** Requests to staff that match the console's filters. */
	let { requests }: { requests: RequestView[] } = $props();
</script>

{#snippet title(request: RequestView)}
	<a class="font-medium text-fg no-underline hover:underline" href="/admin/requests/{request.rkey}">{REQUEST_KIND_LABEL[request.kind]}: {requestSubject(request)}</a>
	{#if request.record.onBehalfOf}<p class="text-label text-fg-subtle">Filed by root on their behalf</p>{/if}
{/snippet}

{#if requests.length}
	<ResponsiveTable rows={requests} key={(request) => request.uri}>
		{#snippet head()}<th>Request</th><th>From</th><th>Submitted</th><th>Status</th>{/snippet}
		{#snippet row(request)}
			<td>{@render title(request)}</td>
			<td><Handle account={request.requester} /></td>
			<td class="whitespace-nowrap text-fg-muted">{timeAgo(request.createdAt)}</td>
			<td><StatusPill status={request.status} /></td>
		{/snippet}
		{#snippet card(request)}
			<div class="flex items-start justify-between gap-3">
				<div class="min-w-0">{@render title(request)}</div>
				<StatusPill status={request.status} />
			</div>
			<p class="text-label text-fg-muted">From <Handle account={request.requester} /> · {timeAgo(request.createdAt)}</p>
		{/snippet}
	</ResponsiveTable>
{:else}
	<EmptyState title="Nothing here" body="No requests match this filter." />
{/if}
