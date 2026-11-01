<script lang="ts">
	import { enhance } from '$app/forms';
	import { Check, Download, ShieldCheck, ShieldX } from '@lucide/svelte';
	import EmptyState from '#lib/components/EmptyState.svelte';
	import Handle from '#lib/components/Handle.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { formFields } from '#lib/form-fields.svelte.ts';
	import { formatDateTime } from '#lib/format.ts';
	import Card from '#lib/ui/Card.svelte';
	import Eyebrow from '#lib/ui/Eyebrow.svelte';
	import ResponsiveTable from '#lib/ui/ResponsiveTable.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const f = $derived(data.filters);

	const fields = $derived(formFields(f));

	// Encoded, so a search with spaces or an ampersand survives the next page.
	const olderHref = $derived(`?${new URLSearchParams({ ...f, before: String(data.entries.at(-1)?.seq) })}`);

	const outcomes = [
		{ value: 'success', label: 'Succeeded' },
		{ value: 'denied', label: 'Denied' },
		{ value: 'failure', label: 'Failed' }
	] as const;

	const stats = $derived([
		{ label: 'Entries', value: data.stats.total },
		{ label: 'Copies pending', value: data.stats.pending },
		{ label: 'Copies failed', value: data.stats.failed },
		{ label: 'Orphan copies', value: data.orphans }
	]);
</script>

{#snippet actor(entry: (typeof data.entries)[number])}
	<Handle account={entry.actor} />{#if entry.actorRole && entry.actorRole !== 'member'}<p class="text-label text-fg-subtle">{entry.actorRole}</p>{/if}
{/snippet}

{#snippet action(entry: (typeof data.entries)[number])}
	<a class="text-fg no-underline hover:underline" href="/admin/audit/{entry.id}">{entry.summary}</a>
	<p class="font-mono text-caption text-fg-subtle">{entry.action}{entry.outcome !== 'success' ? ` · ${entry.outcome}` : ''}</p>
{/snippet}

<!-- Verified copies are the norm, so they stay quiet and the pills mark what still needs attention. -->
{#snippet copy(status: string)}
	{#if status === 'verified'}<span class="inline-flex items-center gap-1 text-label text-fg-subtle"><Check aria-hidden="true" class="size-3.5" />Verified</span>{:else}<StatusPill {status} />{/if}
{/snippet}

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

	<form method="GET" class="flex flex-wrap gap-2 *:min-w-0">
		<input name="q" class="input flex-[2_1_16rem]" bind:value={fields.q} placeholder="Search summaries and subjects" aria-label="Search" />
		<input name="actor" class="input flex-[1_1_12rem]" bind:value={fields.actor} placeholder="Actor handle or DID" aria-label="Actor" />
		<select name="action" class="input flex-[1_1_9rem]" aria-label="Action">
			<option value="">Any action</option>
			{#each data.actionGroups as prefix (prefix)}<option value={prefix} selected={f.action === prefix}>{prefix}.*</option>{/each}
		</select>
		<select name="outcome" class="input flex-[1_1_10rem]" aria-label="Outcome">
			<option value="">Any outcome</option>
			{#each outcomes as outcome (outcome.value)}<option value={outcome.value} selected={f.outcome === outcome.value}>{outcome.label}</option>{/each}
		</select>
		<button class="btn-primary max-sm:grow">Filter</button>
	</form>

	{#if data.entries.length}
		<ResponsiveTable rows={data.entries} key={(entry) => entry.id}>
			{#snippet head()}<th>#</th><th>When</th><th>Actor</th><th>Action</th><th>Actor copy</th><th>Root copy</th>{/snippet}
			{#snippet row(entry)}
				<td class="font-mono text-fg-subtle">{entry.seq}</td>
				<td class="whitespace-nowrap text-fg-muted">{formatDateTime(entry.createdAt)}</td>
				<td>{@render actor(entry)}</td>
				<td>{@render action(entry)}</td>
				<td>{@render copy(entry.actorStatus)}</td>
				<td>{@render copy(entry.rootStatus)}</td>
			{/snippet}
			{#snippet card(entry)}
				<div>{@render action(entry)}</div>
				<div class="flex flex-wrap items-start justify-between gap-x-3 gap-y-1 text-label text-fg-muted">
					<div>{@render actor(entry)}</div>
					<p><span class="font-mono text-fg-subtle">#{entry.seq}</span> · {formatDateTime(entry.createdAt)}</p>
				</div>
				<dl class="grid grid-cols-[auto_1fr] items-center gap-x-3 gap-y-1 text-label">
					<dt class="text-fg-subtle">Actor copy</dt><dd>{@render copy(entry.actorStatus)}</dd>
					<dt class="text-fg-subtle">Root copy</dt><dd>{@render copy(entry.rootStatus)}</dd>
				</dl>
			{/snippet}
		</ResponsiveTable>
	{:else}
		<EmptyState title="Nothing here" body="No entries match these filters." />
	{/if}
	{#if data.entries.length === 100}
		<a class="btn-secondary self-start" href={olderHref}>Older entries</a>
	{/if}
</div>
