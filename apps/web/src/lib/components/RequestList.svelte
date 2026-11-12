<script lang="ts">
	import { requestTitle, timeAgo } from '#lib/format.ts';
	import type { RequestView } from '#lib/types.ts';
	import Button from '#lib/ui/Button.svelte';
	import EmptyState from './EmptyState.svelte';
	import StatusPill from './StatusPill.svelte';

	/** The viewer's own review requests to staff, newest first, each linking to where they can follow or revise it. Sits inside a card. */
	let { requests }: { requests: RequestView[] } = $props();
</script>

{#if requests.length}
	<ul class="m-0 list-none divide-y divide-line p-0">
		{#each requests as request (request.uri)}
			<li class="flex items-center justify-between gap-3 px-5 py-3.5">
				<div class="flex min-w-0 flex-col gap-0.5">
					<a class="truncate text-fg no-underline hover:text-link" href="/dashboard/requests/{request.rkey}">{requestTitle(request)}</a>
					<p class="m-0 text-caption text-fg-subtle">{timeAgo(request.createdAt)}</p>
				</div>
				<StatusPill status={request.status} />
			</li>
		{/each}
	</ul>
{:else}
	<EmptyState framed={false} title="No review requests yet" body="New projects, claims and profile changes you send to the moderators show up here.">
		<Button variant="secondary" size="sm" href="/dashboard/projects/new">List a project</Button>
	</EmptyState>
{/if}
