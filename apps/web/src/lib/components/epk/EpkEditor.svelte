<script lang="ts">
	import { Trash2 } from '@lucide/svelte';
	import Handle from '#lib/components/Handle.svelte';
	import type { EpkConflictView, EpkDraftStatus, EpkFormValues } from '#lib/epk.ts';
	import { formatDateTime } from '#lib/format.ts';
	import type { AccountView } from '#lib/types.ts';
	import DraftStatus from './DraftStatus.svelte';
	import EpkConflicts from './EpkConflicts.svelte';
	import EpkForm from './EpkForm.svelte';

	/**
	 * A press kit's editor: the viewer's draft (or the published press kit when they have none), what the other managers
	 * have open, and saving, discarding or publishing it. A field someone else published in between comes back as
	 * `conflicts`, to settle before publishing.
	 */
	let {
		values,
		photoBase,
		editHref,
		draft,
		others,
		published,
		conflicts
	}: {
		values: EpkFormValues;
		photoBase: string | null;
		editHref: string;
		/** The viewer's draft, if they have one open. */
		draft: { status: EpkDraftStatus; updatedAt: string } | null;
		/** The other managers' open drafts of this press kit. */
		others: { author: AccountView; updatedAt: string }[];
		/** Whether the press kit is published, so it can be deleted. */
		published: boolean;
		conflicts?: EpkConflictView[];
	} = $props();

	let confirming = $state(false);

	const deleted = $derived(draft?.status === 'deleted');
</script>

<div class="flex flex-col gap-6">
	{#if draft || others.length}
		<div class="flex flex-col gap-2">
			{#if draft}
				<p class="m-0 inline-flex flex-wrap items-center gap-2 text-small text-fg-muted"><DraftStatus status={draft.status} />Your draft, saved {formatDateTime(draft.updatedAt)}</p>
			{/if}
			{#if draft?.status === 'behind'}
				<p class="notice m-0">This press kit was published again after you started. Publishing asks about any field you both changed.</p>
			{:else if deleted}
				<p class="notice m-0">This press kit was deleted after you started. Discard the draft, or start a new press kit from it.</p>
			{/if}
			{#each others as other (other.author.did)}
				<p class="notice notice-info m-0"><Handle account={other.author} /> has a draft of this press kit open, saved {formatDateTime(other.updatedAt)}.</p>
			{/each}
		</div>
	{/if}

	{#if conflicts?.length}<EpkConflicts {conflicts} />{/if}

	<EpkForm {values} action={deleted ? '?/save' : '?/publish'} {photoBase} {editHref} submitLabel={deleted ? 'Save draft' : 'Publish'}>
		{#snippet actions()}
			{#if !deleted}<button class="btn-secondary" formaction="?/save">Save draft</button>{/if}
			{#if draft}<button class="btn-ghost" formaction="?/discard" formnovalidate>Discard draft</button>{/if}
			{#if published}
				{#if confirming}
					<span class="text-small text-fg-muted">This deletes it and everyone's access.</span>
					<button class="btn-danger btn-sm" formaction="?/delete" formnovalidate><Trash2 class="size-4" />Delete press kit</button>
					<button type="button" class="btn-ghost btn-sm" onclick={() => (confirming = false)}>Keep it</button>
				{:else}
					<button type="button" class="btn-ghost btn-sm ml-auto" onclick={() => (confirming = true)}><Trash2 class="size-4" />Delete…</button>
				{/if}
			{/if}
		{/snippet}
	</EpkForm>
</div>
