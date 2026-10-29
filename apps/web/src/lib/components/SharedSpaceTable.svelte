<script lang="ts">
	import Handle from '#lib/components/Handle.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { formatDate } from '#lib/format.ts';
	import type { SharedSpaceSummary } from '#lib/types.ts';

	let {
		spaces
	}: {
		/** Null when they couldn't be read; a pending one shows placeholder rows until it arrives. */
		spaces: SharedSpaceSummary[] | null | Promise<SharedSpaceSummary[] | null>;
	} = $props();
</script>

{#await spaces}
	<div class="card flex flex-col gap-3 p-5" aria-busy="true">
		{#each [0, 1, 2] as row (row)}<div class="h-5 animate-pulse rounded-md bg-raised" aria-hidden="true"></div>{/each}
		<span class="sr-only">Loading spaces</span>
	</div>
{:then list}
	{#if !list}
		<div class="card p-5 text-small text-fg-muted">Couldn't load spaces. <a class="link" href="/admin/spaces">Try again</a></div>
	{:else if list.length}
		<!-- Relative, so the visually hidden header stays inside the scrolling box. -->
		<div class="card relative overflow-x-auto">
			<table class="table-base min-w-150">
				<thead><tr><th>Space</th><th>Started by</th><th>People</th><th>Directory</th><th>Started</th></tr></thead>
				<tbody>
					{#each list as space (space.uri)}
						<tr>
							<td>
								<a class="font-medium capitalize text-fg no-underline hover:underline" href="/admin/spaces/{space.kind}/{space.skey}">{space.kind}</a>
								<p class="mono text-fg-subtle">{space.skey}</p>
							</td>
							<td><Handle account={space.createdBy} /></td>
							<td class="font-mono">{space.people}</td>
							<td><StatusPill status={space.listed ? 'listed' : 'unlisted'} /></td>
							<td class="whitespace-nowrap text-fg-muted">{formatDate(space.createdAt)}</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	{:else}
		<div class="card p-5 text-small text-fg-muted">Nobody has started an act, party or venue yet.</div>
	{/if}
{/await}
