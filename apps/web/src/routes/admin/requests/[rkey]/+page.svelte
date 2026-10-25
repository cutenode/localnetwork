<script lang="ts">
	import { enhance } from '$app/forms';
	import { ArrowLeft, TriangleAlert } from '@lucide/svelte';
	import DiffTable from '#lib/components/DiffTable.svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import Handle from '#lib/components/Handle.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { formatDateTime, timeAgo } from '#lib/format.ts';
	import { CLAIM_RELATIONSHIPS, labelFor } from '#lib/taxonomy/options.ts';
	import Card from '#lib/ui/Card.svelte';
	import Eyebrow from '#lib/ui/Eyebrow.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const request = $derived(data.request);

	const own = $derived(request.requester.did === data.viewer?.did);

	let outcome = $state<'approved' | 'rejected' | 'changesRequested'>('approved');

	let busy = $state(false);

	const choices = $derived([
		{ value: 'approved', label: request.kind === 'claim' ? 'Approve and add as manager' : 'Approve and publish' },
		{ value: 'changesRequested', label: 'Ask for changes' },
		{ value: 'rejected', label: 'Reject' }
	] as const);
</script>

<svelte:head><title>Review request · Staff</title></svelte:head>

<div class="flex flex-col gap-6">
	<div>
		<a href="/admin/requests" class="inline-flex items-center gap-1.5 text-small text-fg-muted no-underline hover:text-fg"><ArrowLeft class="size-4" />Queue</a>
		<div class="mt-3 flex flex-wrap items-center gap-x-4 gap-y-3">
			<h1 class="text-heading font-semibold sm:text-title">
				{request.kind === 'create' ? 'New project' : request.kind === 'claim' ? 'Claim' : 'Changes'}:
				{request.project?.name ?? request.record.profile?.name}
			</h1>
			<StatusPill status={request.status} />
		</div>
		<p class="mt-2 text-small text-fg-muted">
			From <Handle account={request.requester} /> · submitted {formatDateTime(request.createdAt)}
			{#if request.record.onBehalfOf}· filed by the root account because their PDS lacks spaces support{/if}
		</p>
	</div>

	<FormMessage {form} />

	<div class="grid grid-cols-[minmax(0,1fr)] items-start gap-6 xl:grid-cols-[minmax(0,1fr)_20rem]">
		<div class="flex min-w-0 flex-col gap-6">
			{#if data.stale}
				<div class="notice flex items-start gap-2">
					<TriangleAlert class="mt-0.5 size-4 shrink-0 text-warning" />
					<p>The project changed since this request was made (revision <span class="font-mono">{request.record.baseRevision}</span> → <span class="font-mono">{data.project?.revision}</span>). Approving replaces the profile with this proposal.</p>
				</div>
			{/if}
			{#if request.kind === 'claim'}
				<Card as="section" class="flex flex-col gap-3 text-small">
					<h2 class="section-title">Claim evidence</h2>
					<p><span class="text-fg-muted">Says they are:</span> <strong class="font-semibold">{labelFor(CLAIM_RELATIONSHIPS, request.record.claim?.relationship)}</strong></p>
					<p class="whitespace-pre-line text-fg-soft">{request.record.claim?.evidence}</p>
					{#if request.record.claim?.links?.length}
						<ul class="list-inside list-disc text-fg-subtle">{#each request.record.claim.links as link, index (index)}<li><a class="link break-all" href={link} rel="noopener noreferrer nofollow" target="_blank">{link}</a></li>{/each}</ul>
					{/if}
					{#if data.project}<p class="text-fg-muted">Current managers: {data.project.managers.length || 'none'}. <a class="link" href="/projects/{data.project.slug}" target="_blank">Open project</a></p>{/if}
				</Card>
			{:else}
				<Card as="section">
					<div class="mb-3 flex items-center justify-between gap-4">
						<h2 class="section-title">{request.kind === 'create' ? 'Proposed profile' : 'Proposed changes'}</h2>
						{#if data.hasAvatar}<img src="/dashboard/requests/{request.rkey}/avatar" alt="Proposed" class="size-16 rounded-lg border border-line object-cover" />{/if}
					</div>
					<DiffTable changes={data.changes} />
					{#if data.tracklists.length}
						<p class="mt-4 text-small">
							<span class="text-fg-muted">Tracklists:</span>
							{#each data.tracklists as t, k (t.n)}{#if k}, {/if}<a class="link" href="/admin/requests/{request.rkey}/mixes/{t.n}">{t.title}</a>{/each}
						</p>
					{/if}
				</Card>
				{#if data.detailChanges.length}
					<Card as="section">
						<h2 class="section-title mb-3">Private details</h2>
						<DiffTable changes={data.detailChanges} />
					</Card>
				{/if}
			{/if}
			{#if request.record.message}
				<Card as="section" class="text-small"><h2 class="section-title mb-2">Note from requester</h2><p class="whitespace-pre-line text-fg-soft">{request.record.message}</p></Card>
			{/if}
			{#if request.decision}
				<Card as="section" class="text-small">
					<h2 class="section-title mb-2">Decision</h2>
					<p><StatusPill status={request.status} /> by <Handle account={request.decision.by} /> on {formatDateTime(request.decision.at)}</p>
					{#if request.decision.reason}<p class="mt-2 whitespace-pre-line text-fg-soft">{request.decision.reason}</p>{/if}
				</Card>
			{/if}
			<Card as="section" class="text-small">
				<h2 class="section-title">Audit trail</h2>
				<ul class="mt-1 divide-y divide-line">
					{#each data.trail as entry (entry.id)}
						<li class="py-2.5"><a class="text-fg no-underline hover:underline" href="/admin/audit/{entry.id}">{entry.summary}</a> <span class="text-fg-subtle">· {timeAgo(entry.createdAt)}</span></li>
					{/each}
				</ul>
			</Card>
		</div>

		<aside class="flex flex-col gap-6">
			{#if request.status === 'pending'}
				{#if own}
					<p class="notice notice-info">This is your own request. Another staff member has to review it.</p>
				{:else}
					<form method="POST" action="?/decide" class="card flex flex-col gap-4 p-5" use:enhance={() => { busy = true; return async ({ update }) => { await update(); busy = false; }; }}>
						<h2 class="text-emphasis font-semibold">Decision</h2>
						<fieldset class="flex flex-col gap-2 text-small">
							<legend class="sr-only">Outcome</legend>
							{#each choices as choice (choice.value)}
								<label class="flex min-h-11 cursor-pointer items-center gap-3 rounded-lg border border-line px-3 transition-colors hover:border-line-strong has-checked:border-fg-subtle has-checked:bg-raised">
									<input type="radio" class="checkbox rounded-full" name="outcome" value={choice.value} bind:group={outcome} />{choice.label}
								</label>
							{/each}
						</fieldset>
						<div>
							<label class="label" for="reason">Message to requester {outcome === 'approved' ? '(optional)' : ''}</label>
							<textarea id="reason" name="reason" class="input min-h-28 py-2.5" required={outcome !== 'approved'} maxlength="1000"></textarea>
						</div>
						<button class="{outcome === 'rejected' ? 'btn-danger' : 'btn-primary'} w-full" disabled={busy}>{busy ? 'Working…' : 'Submit decision'}</button>
						<p class="hint mt-0">Recorded as a decision record by the root account and written to the audit log under your account.</p>
					</form>
				{/if}
			{/if}
			{#if data.verification}
				<Card as="section" class="flex flex-col items-start gap-3 text-small">
					<h2 class="section-title">Bluesky verification</h2>
					{#await data.verification}
						<span class="inline-block h-6 w-24 animate-pulse rounded-full bg-raised" aria-hidden="true"></span><span class="sr-only">Checking</span>
					{:then status}
						{#if !status}
							<p class="text-fg-muted">Couldn't check right now.</p>
						{:else}
							<StatusPill {status} />
						{/if}
						{#if status && status !== 'verified'}
							<a class="btn-secondary w-full" href="/admin/verifications?subject={encodeURIComponent(request.requester.handle ?? request.requester.did)}">{status === 'outdated' ? 'Re-issue verification' : 'Verify on Bluesky'}</a>
						{/if}
					{/await}
				</Card>
			{/if}
			<Card as="section" class="text-small">
				<h2 class="section-title">Requester history</h2>
				<ul class="mt-1 divide-y divide-line">
					{#each data.history as item (item.uri)}
						<li class="flex items-center justify-between gap-2 py-2.5"><a class="truncate text-fg no-underline hover:underline" href="/admin/requests/{item.rkey}">{item.kind}: {item.project?.name ?? item.record.profile?.name}</a><StatusPill status={item.status} /></li>
					{:else}
						<li class="py-2.5 text-fg-muted">First request from this account.</li>
					{/each}
				</ul>
			</Card>
			<Card as="section" class="flex flex-col gap-1.5">
				<Eyebrow>Record</Eyebrow>
				<p class="mono text-fg-subtle">{request.uri}</p>
				<p class="mono text-fg-subtle">cid {request.cid}</p>
			</Card>
		</aside>
	</div>
</div>
