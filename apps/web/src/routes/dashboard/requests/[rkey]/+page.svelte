<script lang="ts">
	import { enhance } from '$app/forms';
	import { ArrowLeft } from '@lucide/svelte';
	import ClaimFields from '#lib/components/ClaimFields.svelte';
	import DiffTable from '#lib/components/DiffTable.svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import Handle from '#lib/components/Handle.svelte';
	import ProjectForm from '#lib/components/ProjectForm.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { formatDateTime } from '#lib/format.ts';
	import { CLAIM_RELATIONSHIPS, labelFor } from '#lib/taxonomy/options.ts';
	import Button from '#lib/ui/Button.svelte';
	import Card from '#lib/ui/Card.svelte';
	import Eyebrow from '#lib/ui/Eyebrow.svelte';
	import type { PageProps } from './$types';
	import { webHref } from '#lib/web-url.ts';

	let { data, form }: PageProps = $props();

	const request = $derived(data.request);

	const editable = $derived(request.requester.did === data.viewer?.did && (request.status === 'pending' || request.status === 'changes_requested'));

	const title = $derived(
		request.kind === 'create'
			? `New project: ${request.record.profile?.name}`
			: request.kind === 'claim'
				? `Claim: ${request.project?.name}`
				: request.kind === 'list'
					? 'Listing in Find DJs'
					: `Changes to ${request.project?.name}`
	);

	let editing = $state(false);

	// A failed revise echoes what was typed; otherwise the form starts from the request.
	const values = $derived(form?.values);

	const claim = $derived(
		// SAFETY: this is only shown for claim requests, and revising a claim echoes `stringValues(form)`, which is all text.
		(values as Record<string, string> | undefined) ?? {
			relationship: request.record.claim?.relationship ?? '',
			evidence: request.record.claim?.evidence ?? '',
			links: request.record.claim?.links?.join('\n') ?? '',
			message: request.record.message ?? ''
		}
	);
</script>

<svelte:head><title>{title} · Local Network</title></svelte:head>

<div class="flex max-w-4xl flex-col gap-6">
	<a href="/dashboard/requests" class="inline-flex items-center gap-1.5 self-start text-small text-fg-muted no-underline hover:text-fg"><ArrowLeft class="size-4" />All requests</a>
	<header class="flex flex-col gap-3">
		<Eyebrow>Request</Eyebrow>
		<div class="flex flex-wrap items-center gap-x-4 gap-y-2">
			<h1 class="m-0 text-title font-semibold">{title}</h1>
			<StatusPill status={request.status} />
		</div>
		<p class="m-0 text-small text-fg-muted">Submitted {formatDateTime(request.createdAt)}{request.updatedAt !== request.createdAt ? ` · revised ${formatDateTime(request.updatedAt)}` : ''}</p>
	</header>

	{#if data.submitted}
		<FormMessage form={{ success: "Submitted. A moderator will review it soon; it was written to your PDS inside Local Network's private moderation space." }} />
	{/if}
	<FormMessage {form} />

	{#if request.decision}
		<Card as="section" class="flex flex-col gap-3">
			<h2 class="m-0 text-emphasis font-semibold">Decision</h2>
			<p class="m-0 text-small text-fg-soft">
				{request.status === 'approved' ? 'Approved' : request.status === 'rejected' ? 'Rejected' : request.status === 'changes_requested' ? 'Changes requested' : 'Decided'}
				{#if request.decision.by} by <Handle account={request.decision.by} />{/if} on {formatDateTime(request.decision.at)}.
			</p>
			{#if request.decision.reason}<blockquote class="m-0 border-l-2 border-line-strong pl-4 text-small whitespace-pre-line text-fg-soft">{request.decision.reason}</blockquote>{/if}
			{#if request.resultProjectSlug}<Button variant="secondary" size="sm" href="/projects/{request.resultProjectSlug}" class="self-start">View project</Button>{/if}
		</Card>
	{/if}

	{#if request.kind === 'claim'}
		<Card as="section" class="flex flex-col gap-3 text-small text-fg-soft">
			<h2 class="m-0 text-emphasis font-semibold text-fg">Your claim</h2>
			<p class="m-0"><span class="text-fg-subtle">Relationship:</span> {labelFor(CLAIM_RELATIONSHIPS, request.record.claim?.relationship)}</p>
			<p class="m-0 whitespace-pre-line">{request.record.claim?.evidence}</p>
			{#if request.record.claim?.links?.length}
				<ul class="m-0 flex list-none flex-col gap-1 p-0">{#each request.record.claim.links as link, index (index)}<li class="break-all"><a class="link" href={webHref(link)} rel="noopener noreferrer nofollow" target="_blank">{link}</a></li>{/each}</ul>
			{/if}
		</Card>
	{:else if request.kind === 'update'}
		<Card as="section" class="flex flex-col gap-3">
			<h2 class="m-0 text-emphasis font-semibold">Proposed changes</h2>
			<DiffTable changes={data.changes} />
		</Card>
	{/if}

	{#if request.record.message}
		<Card as="section" class="flex flex-col gap-2">
			<h2 class="m-0 text-emphasis font-semibold">Your note</h2>
			<p class="m-0 text-small whitespace-pre-line text-fg-soft">{request.record.message}</p>
		</Card>
	{/if}

	{#if editable}
		<div class="flex flex-wrap gap-3">
			{#if request.kind === 'list'}
				<Button variant="secondary" href="/dashboard/profile">Edit your profile</Button>
			{:else}
				<Button variant="secondary" onclick={() => (editing = !editing)}>{editing ? 'Stop editing' : 'Edit request'}</Button>
			{/if}
			<form method="POST" action="?/withdraw" use:enhance={({ cancel }) => { if (!confirm('Withdraw this request?')) cancel(); }}>
				<button class="btn-danger">Withdraw</button>
			</form>
		</div>
		{#if editing}
			{#if request.kind === 'claim'}
				<form method="POST" action="?/revise" class="card flex flex-col gap-4 p-5 sm:p-6">
					<ClaimFields values={claim} />
					<div>
						<label class="label" for="message">Note</label>
						<textarea id="message" name="message" class="input min-h-24 py-2.5">{claim.message ?? ''}</textarea>
					</div>
					<button class="btn-primary self-start">Save and resubmit</button>
				</form>
			{:else}
				<ProjectForm action="?/revise" values={values ?? { ...data.values, message: request.record.message }} sets={data.sets} avatarUrl={data.avatarUrl} showMessage submitLabel="Save and resubmit" geocoder={data.geocoder} />
			{/if}
		{/if}
	{/if}

	<Card as="section" tone="well" padding="sm" class="flex flex-col gap-1">
		<h2 class="m-0 text-small font-medium text-fg-soft">Where this lives</h2>
		<p class="m-0 text-caption text-fg-muted">{request.repoDid === request.requester.did ? 'Your repo' : 'Filed for you by the root account'} inside the moderation space:</p>
		<p class="m-0 font-mono text-caption break-all text-fg-subtle">{request.uri}</p>
	</Card>
</div>
