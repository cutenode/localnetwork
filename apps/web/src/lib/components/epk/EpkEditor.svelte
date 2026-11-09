<script lang="ts">
	import type { ComponentProps } from 'svelte';
	import DeleteRow from '#lib/components/form/DeleteRow.svelte';
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
		form,
		values,
		photoBase,
		editHref,
		draft,
		others,
		published,
		conflicts
	}: {
		form?: ComponentProps<typeof EpkForm>['form'];
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

	<EpkForm {form} {values} action={deleted ? '?/save' : '?/publish'} {photoBase} {editHref} submitLabel={deleted ? 'Save draft' : 'Publish'}>
		{#snippet actions(saving)}
			<!-- First, so pressing Enter in a field saves a draft: publishing takes a click on Publish. -->
			{#if !deleted}<button class="btn-secondary" formaction="?/save" disabled={saving}>Save draft</button>{/if}
		{/snippet}
		{#snippet after(saving)}
			{#if draft}<button class="btn-ghost order-first" formaction="?/discard" formnovalidate disabled={saving}>Discard draft</button>{/if}
		{/snippet}
	</EpkForm>

	{#if published}
		<DeleteRow action="?/delete" label="Delete press kit" note="Deleting takes it off the site and ends everyone's access." warning="This deletes it and everyone's access." />
	{/if}
</div>
