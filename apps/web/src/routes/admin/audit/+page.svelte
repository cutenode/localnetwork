<script lang="ts">
	import { enhance } from '$app/forms';
	import { Download, ShieldCheck, ShieldX } from '@lucide/svelte';
	import Handle from '#lib/components/Handle.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { formatDateTime } from '#lib/format.ts';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const f = $derived(data.filters);
</script>

<svelte:head><title>Audit log · Staff</title></svelte:head>

<div class="flex flex-wrap items-start justify-between gap-4">
	<div>
		<h1 class="text-3xl">Audit log</h1>
		<p class="muted mt-1 max-w-2xl">Every action by every account. Each entry is written into the actor's own repo in the audit space and, with a SHA-256 hash chain, into the root account's repo.</p>
	</div>
	<div class="flex gap-2">
		<form method="POST" action="?/verify" use:enhance><button class="btn-secondary"><ShieldCheck class="size-4" />Verify chain</button></form>
		<a class="btn-secondary" href="/admin/audit/export" download><Download class="size-4" />Export JSONL</a>
	</div>
</div>

{#if form?.verification}
	<div class="notice mt-4 flex items-center gap-2 {form.verification.ok ? 'notice-ok' : 'notice-error'}">
		{#if form.verification.ok}<ShieldCheck class="size-4" />All {form.verification.checked} entries verified: every hash matches and every link is intact.{:else}<ShieldX class="size-4" />Chain broken at entry {form.verification.brokenAt}: {form.verification.problem} ({form.verification.checked} verified before it).{/if}
	</div>
{/if}

<div class="mt-4 grid grid-cols-2 gap-3 sm:grid-cols-4">
	<div class="card px-4 py-3"><p class="muted text-xs">Entries</p><p class="text-2xl">{data.stats.total}</p></div>
	<div class="card px-4 py-3"><p class="muted text-xs">Copies pending</p><p class="text-2xl">{data.stats.pending}</p></div>
	<div class="card px-4 py-3"><p class="muted text-xs">Copies failed</p><p class="text-2xl">{data.stats.failed}</p></div>
	<div class="card px-4 py-3"><p class="muted text-xs">Orphan copies</p><p class="text-2xl">{data.orphans}</p></div>
</div>

<form method="GET" class="mt-6 grid gap-2 sm:grid-cols-[1fr_12rem_10rem_10rem_auto]">
	<input name="q" class="input" value={f.q} placeholder="Search summaries and subjects" />
	<input name="actor" class="input" value={f.actor} placeholder="Actor handle or DID" />
	<select name="action" class="input" aria-label="Action">
		<option value="">Any action</option>
		{#each ['auth', 'project', 'request', 'role', 'booking', 'planner', 'system'] as prefix (prefix)}<option value={prefix} selected={f.action === prefix}>{prefix}.*</option>{/each}
	</select>
	<select name="outcome" class="input" aria-label="Outcome">
		<option value="">Any outcome</option>
		{#each ['success', 'denied', 'failure'] as outcome (outcome)}<option value={outcome} selected={f.outcome === outcome}>{outcome}</option>{/each}
	</select>
	<button class="btn-primary">Filter</button>
</form>

<div class="card mt-6 overflow-x-auto">
	<table class="table-base">
		<thead><tr><th>#</th><th>When</th><th>Actor</th><th>Action</th><th>Actor copy</th><th>Root copy</th></tr></thead>
		<tbody>
			{#each data.entries as entry (entry.id)}
				<tr>
					<td class="mono muted">{entry.seq}</td>
					<td class="whitespace-nowrap">{formatDateTime(entry.createdAt)}</td>
					<td><Handle account={entry.actor} />{#if entry.actorRole && entry.actorRole !== 'member'}<p class="muted text-xs">{entry.actorRole}</p>{/if}</td>
					<td>
						<a class="menu-link" href="/admin/audit/{entry.id}">{entry.summary}</a>
						<p class="mono muted">{entry.action}{entry.outcome !== 'success' ? ` · ${entry.outcome}` : ''}</p>
					</td>
					<td><StatusPill status={entry.actorStatus} /></td>
					<td><StatusPill status={entry.rootStatus} /></td>
				</tr>
			{:else}
				<tr><td colspan="6" class="muted py-8 text-center">No entries.</td></tr>
			{/each}
		</tbody>
	</table>
</div>
{#if data.entries.length === 100}
	<a class="btn-secondary mt-4" href="?q={f.q}&actor={f.actor}&action={f.action}&outcome={f.outcome}&before={data.entries.at(-1)?.seq}">Older entries</a>
{/if}
