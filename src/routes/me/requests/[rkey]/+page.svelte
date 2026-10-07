<script lang="ts">
	import { enhance } from '$app/forms';
	import ClaimFields from '#lib/components/ClaimFields.svelte';
	import DiffTable from '#lib/components/DiffTable.svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import Handle from '#lib/components/Handle.svelte';
	import ProjectForm from '#lib/components/ProjectForm.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { formatDateTime } from '#lib/format.ts';
	import { CLAIM_RELATIONSHIPS, labelFor } from '#lib/taxonomy/options.ts';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const request = $derived(data.request);

	const editable = $derived(request.requester.did === data.viewer?.did && (request.status === 'pending' || request.status === 'changes_requested'));

	const title = $derived(
		request.kind === 'create' ? `New project: ${request.record.profile?.name}` : request.kind === 'claim' ? `Claim: ${request.project?.name}` : `Changes to ${request.project?.name}`
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

<div class="container-page max-w-4xl space-y-6 py-10">
	<div>
		<a href="/me" class="link text-sm">← Dashboard</a>
		<div class="mt-3 flex flex-wrap items-center gap-3">
			<h1 class="text-3xl">{title}</h1>
			<StatusPill status={request.status} />
		</div>
		<p class="muted mt-1 text-sm">Submitted {formatDateTime(request.createdAt)}{request.updatedAt !== request.createdAt ? ` · revised ${formatDateTime(request.updatedAt)}` : ''}</p>
	</div>

	{#if data.submitted}
		<FormMessage form={{ success: "Submitted. A moderator will review it soon; it was written to your PDS inside Local Network's private moderation space." }} />
	{/if}
	<FormMessage {form} />

	{#if request.decision}
		<section class="card card-pad">
			<h2 class="section-title mb-2">Decision</h2>
			<p class="text-sm">
				{request.status === 'approved' ? 'Approved' : request.status === 'rejected' ? 'Rejected' : request.status === 'changes_requested' ? 'Changes requested' : 'Decided'}
				{#if request.decision.by} by <Handle account={request.decision.by} />{/if} on {formatDateTime(request.decision.at)}.
			</p>
			{#if request.decision.reason}<blockquote class="mt-3 border-l-4 border-ansi-magenta pl-4 text-sm whitespace-pre-line">{request.decision.reason}</blockquote>{/if}
			{#if request.resultProjectSlug}<a class="btn-secondary btn-sm mt-4" href="/projects/{request.resultProjectSlug}">View project</a>{/if}
		</section>
	{/if}

	{#if request.kind === 'claim'}
		<section class="card card-pad space-y-3 text-sm">
			<h2 class="section-title">Your claim</h2>
			<p><span class="muted">Relationship:</span> {labelFor(CLAIM_RELATIONSHIPS, request.record.claim?.relationship)}</p>
			<p class="whitespace-pre-line">{request.record.claim?.evidence}</p>
			{#if request.record.claim?.links?.length}
				<ul class="list-inside list-disc">{#each request.record.claim.links as link, index (index)}<li><a class="link" href={link} rel="noopener noreferrer nofollow" target="_blank">{link}</a></li>{/each}</ul>
			{/if}
		</section>
	{:else if request.kind === 'update'}
		<section class="card card-pad">
			<h2 class="section-title mb-3">Proposed changes</h2>
			<DiffTable changes={data.changes} />
		</section>
	{/if}

	{#if request.record.message}
		<section class="card card-pad text-sm"><h2 class="section-title mb-2">Your note</h2><p class="whitespace-pre-line">{request.record.message}</p></section>
	{/if}

	{#if editable}
		<div class="flex flex-wrap gap-3">
			<button class="btn-secondary" onclick={() => (editing = !editing)}>{editing ? 'Stop editing' : 'Edit request'}</button>
			<form method="POST" action="?/withdraw" use:enhance={({ cancel }) => { if (!confirm('Withdraw this request?')) cancel(); }}>
				<button class="btn-ghost text-ansi-bright-red">Withdraw</button>
			</form>
		</div>
		{#if editing}
			{#if request.kind === 'claim'}
				<form method="POST" action="?/revise" class="card card-pad space-y-4">
					<ClaimFields values={claim} />
					<div>
						<label class="label" for="message">Note</label>
						<textarea id="message" name="message" class="input">{claim.message ?? ''}</textarea>
					</div>
					<button class="btn-accent">Save and resubmit</button>
				</form>
			{:else}
				<ProjectForm action="?/revise" values={values ?? { ...data.values, message: request.record.message }} avatarUrl={data.avatarUrl} showMessage submitLabel="Save and resubmit" geocoder={data.geocoder} />
			{/if}
		{/if}
	{/if}

	<section class="card card-pad space-y-1 text-xs">
		<h2 class="text-sm">Where this lives</h2>
		<p class="muted">{request.repoDid === request.requester.did ? 'Your repo' : 'Filed on your behalf by the root account'} inside the moderation space:</p>
		<p class="mono muted">{request.uri}</p>
	</section>
</div>
