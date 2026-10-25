<script lang="ts">
	import EmptyState from '#lib/components/EmptyState.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { formatDateTime } from '#lib/format.ts';
	import Button from '#lib/ui/Button.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<svelte:head><title>Activity · Local Network</title></svelte:head>

<div class="flex max-w-4xl flex-col gap-8">
	<SectionHeader
		level={1}
		eyebrow="Audit log"
		title="Your activity"
		lede="Every action you take is audited. Each entry goes into your own repo inside Local Network's audit space, and the root account writes a hash-chained copy."
	/>
	{#if data.entries.length === 0}
		<EmptyState title="Nothing yet" body="Requests, bookings and planner changes show up here." />
	{:else}
		<ol class="card m-0 list-none divide-y divide-line p-0">
			{#each data.entries as entry (entry.id)}
				<li class="flex flex-col gap-3 px-5 py-4 sm:flex-row sm:items-start sm:justify-between sm:gap-6">
					<div class="flex min-w-0 flex-col gap-1">
						<p class="m-0 text-fg">{entry.summary}</p>
						<p class="m-0 font-mono text-caption break-all text-fg-subtle">{entry.action}</p>
						<time class="text-caption text-fg-subtle" datetime={entry.createdAt}>{formatDateTime(entry.createdAt)}</time>
					</div>
					<dl class="m-0 flex shrink-0 flex-wrap gap-x-5 gap-y-1.5 text-caption sm:flex-col sm:items-end">
						<div class="flex items-center gap-2"><dt class="text-fg-subtle">Your copy</dt><dd class="m-0"><StatusPill status={entry.actorStatus} /></dd></div>
						<div class="flex items-center gap-2"><dt class="text-fg-subtle">Root copy</dt><dd class="m-0"><StatusPill status={entry.rootStatus} /></dd></div>
					</dl>
				</li>
			{/each}
		</ol>
	{/if}
	{#if data.entries.length === 50}
		<Button variant="secondary" href="?before={data.entries.at(-1)?.seq}" class="self-start">Older</Button>
	{/if}
</div>
