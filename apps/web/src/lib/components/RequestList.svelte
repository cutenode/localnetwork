<script lang="ts">
	import { timeAgo } from '#lib/format.ts';
	import type { RequestView } from '#lib/types.ts';
	import StatusPill from './StatusPill.svelte';

	/** The viewer's own requests to staff, newest first, each linking to where they can follow or revise it. */
	let { requests }: { requests: RequestView[] } = $props();

	const kindLabel = { create: 'New project', claim: 'Claim', update: 'Changes' } as const;
</script>

{#if requests.length}
	<ul class="m-0 list-none divide-y divide-line p-0">
		{#each requests as request (request.uri)}
			<li class="flex items-center justify-between gap-3 px-5 py-3.5">
				<div class="flex min-w-0 flex-col gap-0.5">
					<a class="truncate text-fg no-underline hover:text-link" href="/dashboard/requests/{request.rkey}">{kindLabel[request.kind]}: {request.project?.name ?? request.record.profile?.name ?? 'Project'}</a>
					<p class="m-0 text-caption text-fg-subtle">{timeAgo(request.createdAt)}</p>
				</div>
				<StatusPill status={request.status} />
			</li>
		{/each}
	</ul>
{:else}
	<p class="m-0 px-5 py-6 text-small text-fg-muted">No requests yet.</p>
{/if}
