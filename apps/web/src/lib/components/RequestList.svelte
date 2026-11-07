<script lang="ts">
	import { requestStanding, requestTitle } from '#lib/format.ts';
	import type { RequestView } from '#lib/types.ts';
	import Button from '#lib/ui/Button.svelte';
	import EmptyState from './EmptyState.svelte';

	/**
	 * The viewer's own requests to staff, the ones back with them first, each saying where it stands and linking to where
	 * they can follow or revise it. `loading` holds the room of a few.
	 */
	let { requests = [], loading = false }: { requests?: RequestView[]; loading?: boolean } = $props();

	const rank = ({ status }: RequestView) => ['changes_requested', 'pending'].indexOf(status) + 1 || 3;

	const ordered = $derived(requests.toSorted((a, b) => rank(a) - rank(b)));
</script>

{#if loading || requests.length}
	<ul class="card m-0 list-none divide-y divide-line overflow-hidden p-0" aria-busy={loading || undefined}>
		{#if loading}
			{#each { length: 3 }, row (row)}
				<li class="flex flex-col gap-0.5 px-4 py-3 sm:px-5 sm:py-3.5">
					<span class="flex min-h-lh items-center text-small"><span class="h-3.5 w-2/5 rounded-md bg-well motion-safe:animate-pulse"></span></span>
					<span class="flex min-h-lh items-center text-label"><span class="h-3 w-3/5 rounded-md bg-well motion-safe:animate-pulse"></span></span>
				</li>
			{/each}
		{/if}
		{#each ordered as request (request.uri)}
			{@const isYourTurn = request.status === 'changes_requested'}
			<li class="relative flex items-center gap-3 px-4 py-3 transition-colors hover:bg-raised sm:gap-4 sm:px-5 sm:py-3.5">
				<div class="flex min-w-0 flex-1 flex-col gap-0.5">
					{#if isYourTurn}
						<p class="m-0 text-small font-medium break-words text-fg">{requestTitle(request)}</p>
					{:else}
						<a class="text-small font-medium break-words text-fg no-underline after:absolute after:inset-0" href="/dashboard/account/requests/{request.rkey}">{requestTitle(request)}</a>
					{/if}
					<p class="m-0 text-label text-fg-muted">{requestStanding(request)}</p>
				</div>
				{#if isYourTurn}<Button href="/dashboard/account/requests/{request.rkey}" size="sm" variant="secondary" aria-label="Update: {requestTitle(request)}" class="shrink-0 after:absolute after:inset-0">Update</Button>{/if}
			</li>
		{/each}
	</ul>
{:else}
	<EmptyState title="No requests yet" body="New projects, claims and profile changes you ask staff for show up here.">
		<Button variant="secondary" size="sm" href="/dashboard/projects/new">List a project</Button>
	</EmptyState>
{/if}
