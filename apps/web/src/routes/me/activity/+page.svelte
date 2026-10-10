<script lang="ts">
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { formatDateTime } from '#lib/format.ts';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<svelte:head><title>My activity · Local Network</title></svelte:head>

<div class="container-page max-w-4xl py-10">
	<a href="/me" class="link text-sm">← Dashboard</a>
	<h1 class="mt-3 text-3xl">Your activity</h1>
	<p class="muted mt-2 max-w-2xl">Every action you take is audited. Each entry is written into your own repo inside Local Network's audit space, and a hash-chained copy is written by the root account.</p>
	<div class="card mt-8 overflow-x-auto">
		<table class="table-base">
			<thead><tr><th>When</th><th>Action</th><th>Your copy</th><th>Root copy</th></tr></thead>
			<tbody>
				{#each data.entries as entry (entry.id)}
					<tr>
						<td class="whitespace-nowrap">{formatDateTime(entry.createdAt)}</td>
						<td><p class="text-ansi-white">{entry.summary}</p><p class="mono muted">{entry.action}</p></td>
						<td><StatusPill status={entry.actorStatus} /></td>
						<td><StatusPill status={entry.rootStatus} /></td>
					</tr>
				{:else}
					<tr><td colspan="4" class="muted py-8 text-center">Nothing yet.</td></tr>
				{/each}
			</tbody>
		</table>
	</div>
	{#if data.entries.length === 50}
		<a class="btn-secondary mt-4" href="?before={data.entries.at(-1)?.seq}">Older</a>
	{/if}
</div>
