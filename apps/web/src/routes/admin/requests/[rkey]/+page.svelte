<script lang="ts">
	import { enhance } from '$app/forms';
	import { TriangleAlert } from '@lucide/svelte';
	import DiffTable from '#lib/components/DiffTable.svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import Handle from '#lib/components/Handle.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { formatDateTime, timeAgo } from '#lib/format.ts';
	import { CLAIM_RELATIONSHIPS, labelFor } from '#lib/taxonomy/options.ts';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const request = $derived(data.request);

	const own = $derived(request.requester.did === data.viewer?.did);

	let outcome = $state<'approved' | 'rejected' | 'changesRequested'>('approved');

	let busy = $state(false);
</script>

<svelte:head><title>Review request · Staff</title></svelte:head>

<a href="/admin/requests" class="link text-sm">← Queue</a>
<div class="mt-3 flex flex-wrap items-center gap-3">
	<h1 class="text-3xl">
		{request.kind === 'create' ? 'New project' : request.kind === 'claim' ? 'Claim' : 'Changes'}:
		{request.project?.name ?? request.record.profile?.name}
	</h1>
	<StatusPill status={request.status} />
</div>
<p class="muted mt-1 text-sm">
	From <Handle account={request.requester} /> · submitted {formatDateTime(request.createdAt)}
	{#if request.record.onBehalfOf}· filed by the root account because their PDS lacks spaces support{/if}
</p>

<div class="mt-6 space-y-4"><FormMessage {form} /></div>

<div class="mt-6 grid gap-6 xl:grid-cols-[1fr_22rem]">
	<div class="space-y-6">
		{#if data.stale}
			<div class="flex gap-2 notice">
				<TriangleAlert class="size-4 shrink-0" />The project changed since this request was made (revision {request.record.baseRevision} → {data.project?.revision}). Approving replaces the profile with this proposal.
			</div>
		{/if}
		{#if request.kind === 'claim'}
			<section class="card card-pad space-y-3 text-sm">
				<h2 class="section-title">Claim evidence</h2>
				<p><span class="muted">Says they are:</span> <strong>{labelFor(CLAIM_RELATIONSHIPS, request.record.claim?.relationship)}</strong></p>
				<p class="whitespace-pre-line">{request.record.claim?.evidence}</p>
				{#if request.record.claim?.links?.length}
					<ul class="list-inside list-disc">{#each request.record.claim.links as link, index (index)}<li><a class="link" href={link} rel="noopener noreferrer nofollow" target="_blank">{link}</a></li>{/each}</ul>
				{/if}
				{#if data.project}<p class="muted">Current managers: {data.project.managers.length || 'none'}. <a class="link" href="/projects/{data.project.slug}" target="_blank">Open project</a></p>{/if}
			</section>
		{:else}
			<section class="card card-pad">
				<div class="mb-3 flex items-center justify-between gap-4">
					<h2 class="section-title">{request.kind === 'create' ? 'Proposed profile' : 'Proposed changes'}</h2>
					{#if data.hasAvatar}<img src="/me/requests/{request.rkey}/avatar" alt="Proposed" class="size-16 border-2 border-ansi-cyan object-cover" />{/if}
				</div>
				<DiffTable changes={data.changes} />
				{#if data.tracklists.length}
					<p class="mt-3 text-sm">
						<span class="muted">Tracklists:</span>
						{#each data.tracklists as t, k (t.n)}{#if k}, {/if}<a class="link" href="/admin/requests/{request.rkey}/mixes/{t.n}">{t.title}</a>{/each}
					</p>
				{/if}
			</section>
			{#if data.detailChanges.length}
				<section class="card card-pad">
					<h2 class="section-title mb-3">Private details</h2>
					<DiffTable changes={data.detailChanges} />
				</section>
			{/if}
		{/if}
		{#if request.record.message}
			<section class="card card-pad text-sm"><h2 class="section-title mb-2">Note from requester</h2><p class="whitespace-pre-line">{request.record.message}</p></section>
		{/if}
		{#if request.decision}
			<section class="card card-pad text-sm">
				<h2 class="section-title mb-2">Decision</h2>
				<p>{request.status} by <Handle account={request.decision.by} /> on {formatDateTime(request.decision.at)}</p>
				{#if request.decision.reason}<p class="mt-2 whitespace-pre-line">{request.decision.reason}</p>{/if}
			</section>
		{/if}
		<section class="card card-pad">
			<h2 class="section-title mb-3">Audit trail</h2>
			<ul class="space-y-2 text-sm">
				{#each data.trail as entry (entry.id)}
					<li><a class="hover:underline" href="/admin/audit/{entry.id}">{entry.summary}</a> <span class="muted">· {timeAgo(entry.createdAt)}</span></li>
				{/each}
			</ul>
		</section>
	</div>

	<aside class="space-y-6">
		{#if request.status === 'pending'}
			{#if own}
				<div class="card card-pad text-sm">This is your own request. Another staff member has to review it.</div>
			{:else}
				<form method="POST" action="?/decide" class="card card-pad space-y-4" use:enhance={() => { busy = true; return async ({ update }) => { await update(); busy = false; }; }}>
					<h2 class="section-title">Decision</h2>
					<fieldset class="space-y-2 text-sm">
						<label class="flex items-center gap-2"><input type="radio" class="checkbox rounded-full" name="outcome" value="approved" bind:group={outcome} />Approve{request.kind === 'claim' ? ' and add as manager' : ' and publish'}</label>
						<label class="flex items-center gap-2"><input type="radio" class="checkbox rounded-full" name="outcome" value="changesRequested" bind:group={outcome} />Ask for changes</label>
						<label class="flex items-center gap-2"><input type="radio" class="checkbox rounded-full" name="outcome" value="rejected" bind:group={outcome} />Reject</label>
					</fieldset>
					<div>
						<label class="label" for="reason">Message to requester {outcome === 'approved' ? '(optional)' : ''}</label>
						<textarea id="reason" name="reason" class="input min-h-28" required={outcome !== 'approved'} maxlength="1000"></textarea>
					</div>
					<button class="{outcome === 'rejected' ? 'btn-danger' : 'btn-accent'} w-full" disabled={busy}>{busy ? 'Working…' : 'Submit decision'}</button>
					<p class="hint">Recorded as a decision record by the root account and written to the audit log under your account.</p>
				</form>
			{/if}
		{/if}
		<section class="card card-pad text-sm">
			<h2 class="section-title mb-3">Requester history</h2>
			<ul class="space-y-2">
				{#each data.history as item (item.uri)}
					<li class="flex items-center justify-between gap-2"><a class="truncate hover:underline" href="/admin/requests/{item.rkey}">{item.kind}: {item.project?.name ?? item.record.profile?.name}</a><StatusPill status={item.status} /></li>
				{:else}
					<li class="muted">First request from this account.</li>
				{/each}
			</ul>
		</section>
		<section class="card card-pad space-y-1 text-xs"><h2 class="text-sm">Record</h2><p class="mono muted">{request.uri}</p><p class="mono muted">cid {request.cid}</p></section>
	</aside>
</div>
