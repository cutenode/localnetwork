<script lang="ts">
	import { ArrowLeft } from '@lucide/svelte';
	import { enhance } from '$app/forms';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import Handle from '#lib/components/Handle.svelte';
	import { AUDIENCE_LABELS, AUDIENCES, presetFor, ROLE_LABELS, RULE_LABELS } from '#lib/epk.ts';
	import { formatDate } from '#lib/format.ts';
	import { control } from '#lib/ui/Field.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const base = $derived(`/epk/${data.epk.did}/${data.epk.rkey}`);

	const pending = $derived(data.requests.filter((request) => request.status === 'pending'));

	const declined = $derived(data.requests.filter((request) => request.status === 'declined'));

	const preset = $derived(presetFor(data.epk.record.access));

	const VIA = { auto: 'Let in automatically', approved: 'You approved', invited: 'You added' } as const;
</script>

<svelte:head><title>Who can see {data.epk.record.title} · Local Network</title></svelte:head>

<div class="container-page flex max-w-208 flex-col gap-10 py-10 sm:py-14">
	<a href={base} class="menu-link inline-flex w-fit items-center gap-1.5 text-small"><ArrowLeft class="size-4" />Back to the press kit</a>
	<div class="flex flex-col gap-2">
		<p class="m-0 eyebrow text-fg-subtle">{data.project.profile.name} · {data.epk.record.title}</p>
		<h1 class="m-0 text-title font-semibold">Who can see the rest</h1>
		<p class="m-0 text-fg-muted">
			{preset ? preset.label : 'Your own mix'}.
			<a class="link" href="{base}/edit">Change the rules</a>
		</p>
		{#if !preset}
			<ul class="m-0 flex list-none flex-col gap-1 p-0 text-small text-fg-muted">
				{#each AUDIENCES as audience (audience)}<li>{AUDIENCE_LABELS[audience].label}: {RULE_LABELS[data.epk.record.access[audience]].toLowerCase()}</li>{/each}
			</ul>
		{/if}
	</div>

	<FormMessage {form} />

	<section aria-labelledby="waiting" class="flex flex-col gap-3">
		<h2 id="waiting" class="m-0 text-heading font-semibold">Waiting for you</h2>
		{#if pending.length}
			<ul class="m-0 flex list-none flex-col p-0">
				{#each pending as request (request.uri)}
					<li class="flex flex-wrap items-start justify-between gap-3 border-t border-line py-4 last:border-b">
						<div class="flex min-w-0 flex-col gap-1">
							<p class="m-0 text-small"><Handle account={request.requester} />{#if request.role}{' '}<span class="text-fg-muted">· {ROLE_LABELS[request.role]}</span>{/if}</p>
							{#if request.message}<p class="m-0 text-small text-fg-soft">{request.message}</p>{/if}
							<p class="m-0 text-caption text-fg-subtle">Asked {formatDate(request.createdAt)}{request.rule === 'auto' ? ' · approved by your rules, saved once you sign in again' : ''}</p>
						</div>
						<form method="POST" class="flex gap-2" use:enhance>
							<input type="hidden" name="request" value={request.uri} />
							<button class="btn-primary btn-sm" formaction="?/approve">Let in</button>
							<button class="btn-ghost btn-sm" formaction="?/decline">Decline</button>
						</form>
					</li>
				{/each}
			</ul>
		{:else}
			<p class="m-0 text-small text-fg-muted">No requests waiting.</p>
		{/if}
	</section>

	<section aria-labelledby="granted" class="flex flex-col gap-3">
		<h2 id="granted" class="m-0 text-heading font-semibold">People with access</h2>
		{#if data.grants.length}
			<ul class="m-0 flex list-none flex-col p-0">
				{#each data.grants as grant (grant.subject.did)}
					<li class="flex flex-wrap items-center justify-between gap-3 border-t border-line py-3 last:border-b">
						<p class="m-0 flex flex-col text-small">
							<Handle account={grant.subject} />
							<span class="text-caption text-fg-subtle">{VIA[grant.via]} · {formatDate(grant.createdAt)}</span>
						</p>
						<form method="POST" action="?/revoke" use:enhance>
							<input type="hidden" name="subject" value={grant.subject.did} />
							<button class="btn-ghost btn-sm">Remove</button>
						</form>
					</li>
				{/each}
			</ul>
		{:else}
			<p class="m-0 text-small text-fg-muted">Nobody yet.</p>
		{/if}
		<form method="POST" action="?/invite" class="flex flex-wrap items-end gap-2" use:enhance>
			<div class="flex min-w-60 flex-1 flex-col gap-1.5">
				<label for="handle" class="text-label font-medium text-fg-soft">Add someone</label>
				<input id="handle" name="handle" placeholder="promoter.bsky.social" class={control} value={form && 'values' in form ? (form.values?.handle ?? '') : ''} />
			</div>
			<button class="btn-secondary">Add</button>
		</form>
	</section>

	{#if declined.length}
		<section aria-labelledby="declined" class="flex flex-col gap-3">
			<h2 id="declined" class="m-0 text-emphasis font-semibold">Declined</h2>
			<ul class="m-0 flex list-none flex-col gap-1 p-0 text-small text-fg-muted">
				{#each declined as request (request.uri)}<li><Handle account={request.requester} /> · {formatDate(request.decidedAt ?? request.createdAt)}</li>{/each}
			</ul>
		</section>
	{/if}

	<p class="m-0 text-caption text-fg-subtle">Each person you let in gets a grant record in this press kit's space on your PDS, and a place on its member list. Removing them deletes both.</p>
</div>
