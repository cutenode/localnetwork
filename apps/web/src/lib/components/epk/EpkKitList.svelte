<script lang="ts">
	import { ExternalLink, KeyRound, Lock, Pencil, Plus } from '@lucide/svelte';
	import EmptyState from '#lib/components/EmptyState.svelte';
	import Handle from '#lib/components/Handle.svelte';
	import { type EpkDashboard, type EpkDraftView, presetFor, pressKitPath } from '#lib/epk.ts';
	import { formatDate } from '#lib/format.ts';
	import DraftStatus from './DraftStatus.svelte';

	/**
	 * A project's press kits on its dashboard: each with how it's shared and every manager's open draft of it, then drafts
	 * of press kits that aren't published. Only the viewer's own drafts of unpublished ones open, since nobody else's
	 * changes have anything to show over.
	 */
	let { dashboard, slug, viewerDid }: { dashboard: Promise<EpkDashboard>; slug: string; viewerDid: string } = $props();

	const editor = (rkey: string) => `/dashboard/projects/${slug}/press-kits/${rkey}`;
</script>

{#snippet drafts(list: EpkDraftView[])}
	<ul class="m-0 flex list-none flex-col gap-1.5 p-0" aria-label="Open drafts">
		{#each list as draft (draft.uri)}
			<li class="flex flex-wrap items-center gap-x-2 gap-y-1 text-caption text-fg-muted">
				<DraftStatus status={draft.status} />
				<span>{#if draft.author.did === viewerDid}Your draft{:else}<Handle account={draft.author} />'s draft{/if} · saved {formatDate(draft.updatedAt)}</span>
			</li>
		{/each}
	</ul>
{/snippet}

{#await dashboard}
	<div role="status" aria-busy="true" aria-label="Loading press kits" class="flex flex-col gap-3">
		<div class="h-24 animate-pulse rounded-xl bg-surface"></div>
		<div class="h-24 animate-pulse rounded-xl bg-surface"></div>
	</div>
{:then { kits, drafts: unpublished }}
	{#if kits.length || unpublished.length}
		{#if kits.length}
			<ul class="m-0 flex list-none flex-col gap-3 p-0">
				{#each kits as { epk, drafts: open } (epk.uri)}
					{@const preset = presetFor(epk.record.access)}
					{@const mine = open.some((draft) => draft.author.did === viewerDid)}
					<li class="card flex flex-wrap items-start justify-between gap-x-6 gap-y-3 p-4 sm:px-5">
						<div class="flex min-w-0 flex-col gap-1.5">
							<a class="text-emphasis font-semibold text-fg no-underline hover:underline" href={editor(epk.rkey)}>{epk.record.title}</a>
							{#if epk.record.headline}<p class="m-0 text-small text-fg-soft">{epk.record.headline}</p>{/if}
							<p class="m-0 inline-flex flex-wrap items-center gap-1.5 text-caption text-fg-subtle"><Lock class="size-3" aria-hidden="true" />{preset?.label ?? 'Custom access'} · Updated {formatDate(epk.record.updatedAt ?? epk.record.createdAt)}{#if epk.legacy}{' '}· Moves to the project on its maker's next visit{/if}</p>
							{#if open.length}{@render drafts(open)}{/if}
						</div>
						<div class="flex flex-wrap gap-2">
							{#if !epk.legacy}
								<a class="btn-secondary btn-sm" href={editor(epk.rkey)} aria-label="{mine ? 'Continue draft of' : 'Edit'} {epk.record.title}"><Pencil class="size-4" />{mine ? 'Continue draft' : 'Edit'}</a>
								<a class="btn-secondary btn-sm" href="{editor(epk.rkey)}/access" aria-label="Access to {epk.record.title}"><KeyRound class="size-4" />Access</a>
							{/if}
							<a class="btn-ghost btn-sm" href={pressKitPath(slug, epk.rkey)} aria-label="View {epk.record.title}"><ExternalLink class="size-4" />View</a>
						</div>
					</li>
				{/each}
			</ul>
		{/if}
		{#if unpublished.length}
			<section aria-labelledby="unpublished" class="flex flex-col gap-3">
				<h2 id="unpublished" class="m-0 text-emphasis font-semibold">Not published</h2>
				<ul class="m-0 flex list-none flex-col gap-3 p-0">
					{#each unpublished as draft (draft.uri)}
						{@const title = draft.changes.title ?? 'Untitled press kit'}
						<li class="card flex flex-wrap items-center justify-between gap-3 p-4 sm:px-5">
							<div class="flex min-w-0 flex-col gap-1.5">
								<p class="m-0 text-emphasis font-semibold">{title}</p>
								{@render drafts([draft])}
							</div>
							{#if draft.author.did === viewerDid}<a class="btn-secondary btn-sm" href={editor(draft.rkey)} aria-label="Continue draft of {title}"><Pencil class="size-4" />Continue draft</a>{/if}
						</li>
					{/each}
				</ul>
			</section>
		{/if}
	{:else}
		<EmptyState title="No press kits yet" body="Make one for each kind of booking, like clubs or festivals.">
			<a class="btn-primary btn-sm" href="/dashboard/projects/{slug}/press-kits/new"><Plus class="size-4" />New press kit</a>
		</EmptyState>
	{/if}
{:catch}
	<p role="alert" class="notice notice-error m-0">Couldn't load the press kits. Reload the page to try again.</p>
{/await}
