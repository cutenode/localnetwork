<script lang="ts">
	import { enhance } from '$app/forms';
	import { Download, ShieldCheck, ShieldX } from '@lucide/svelte';
	import Handle from '#lib/components/Handle.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { formatDateTime } from '#lib/format.ts';
	import Card from '#lib/ui/Card.svelte';
	import Eyebrow from '#lib/ui/Eyebrow.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const f = $derived(data.filters);

	// Encoded, so a search with spaces or an ampersand survives the next page.
	const olderHref = $derived(`?${new URLSearchParams({ ...f, before: String(data.entries.at(-1)?.seq) })}`);

	const stats = $derived([
		{ label: 'Entries', value: data.stats.total },
		{ label: 'Copies pending', value: data.stats.pending },
		{ label: 'Copies failed', value: data.stats.failed },
		{ label: 'Orphan copies', value: data.orphans }
	]);
</script>

<svelte:head><title>Audit log · Staff</title></svelte:head>

<div class="flex flex-col gap-6">
	<SectionHeader level={1} title="Audit log" lede="Every action by every account. Each entry is written to the actor's own repo in the audit space, and to the root account's repo with a SHA-256 hash chain.">
		{#snippet actions()}
			<form method="POST" action="?/verify" use:enhance><button class="btn-secondary w-full"><ShieldCheck class="size-4" />Verify chain</button></form>
			<a class="btn-secondary" href="/admin/audit/export" download><Download class="size-4" />Export JSONL</a>
		{/snippet}
	</SectionHeader>

	{#if form?.verification}
		<div role="status" class="notice flex items-start gap-2 {form.verification.ok ? 'notice-ok' : 'notice-error'}">
			{#if form.verification.ok}<ShieldCheck class="mt-0.5 size-4 shrink-0 text-good" />All {form.verification.checked} entries verified: every hash matches and every link is intact.{:else}<ShieldX class="mt-0.5 size-4 shrink-0 text-critical" />Chain broken at entry {form.verification.brokenAt}: {form.verification.problem} ({form.verification.checked} verified before it).{/if}
		</div>
	{/if}

	<div class="grid grid-cols-2 gap-3 sm:grid-cols-4">
		{#each stats as stat (stat.label)}
			<Card padding="sm"><Eyebrow>{stat.label}</Eyebrow><p class="mt-2 font-mono text-heading text-fg">{stat.value}</p></Card>
		{/each}
	</div>

	<form method="GET" class="grid gap-2 sm:grid-cols-2 xl:grid-cols-[minmax(0,1fr)_12rem_9rem_9rem_auto]">
		<input name="q" class="input sm:col-span-2 xl:col-span-1" value={f.q} placeholder="Search summaries and subjects" aria-label="Search" />
		<input name="actor" class="input" value={f.actor} placeholder="Actor handle or DID" aria-label="Actor" />
		<select name="action" class="input" aria-label="Action">
			<option value="">Any action</option>
			{#each data.actionGroups as prefix (prefix)}<option value={prefix} selected={f.action === prefix}>{prefix}.*</option>{/each}
		</select>
		<select name="outcome" class="input" aria-label="Outcome">
			<option value="">Any outcome</option>
			{#each ['success', 'denied', 'failure'] as outcome (outcome)}<option value={outcome} selected={f.outcome === outcome}>{outcome}</option>{/each}
		</select>
		<button class="btn-primary">Filter</button>
	</form>

	<div class="card overflow-x-auto">
		<table class="table-base min-w-180">
			<thead><tr><th>#</th><th>When</th><th>Actor</th><th>Action</th><th>Actor copy</th><th>Root copy</th></tr></thead>
			<tbody>
				{#each data.entries as entry (entry.id)}
					<tr>
						<td class="font-mono text-fg-subtle">{entry.seq}</td>
						<td class="whitespace-nowrap text-fg-muted">{formatDateTime(entry.createdAt)}</td>
						<td><Handle account={entry.actor} />{#if entry.actorRole && entry.actorRole !== 'member'}<p class="text-label text-fg-subtle">{entry.actorRole}</p>{/if}</td>
						<td>
							<a class="text-fg no-underline hover:underline" href="/admin/audit/{entry.id}">{entry.summary}</a>
							<p class="font-mono text-caption text-fg-subtle">{entry.action}{entry.outcome !== 'success' ? ` · ${entry.outcome}` : ''}</p>
						</td>
						<td><StatusPill status={entry.actorStatus} /></td>
						<td><StatusPill status={entry.rootStatus} /></td>
					</tr>
				{:else}
					<tr><td colspan="6" class="py-8 text-center text-fg-muted">No entries match these filters.</td></tr>
				{/each}
			</tbody>
		</table>
	</div>
	{#if data.entries.length === 100}
		<a class="btn-secondary self-start" href={olderHref}>Older entries</a>
	{/if}
</div>
