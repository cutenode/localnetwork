<script lang="ts">
	import { CircleAlert, Ellipsis, Eye, LoaderCircle, Pencil, ShieldCheck } from '@lucide/svelte';
	import type { Snippet } from 'svelte';
	import type { EditActivity, EditPhase, EditRole, PageStatus } from './types.ts';

	/**
	 * The one bar for everyone who can change the page they're on: its people, and staff. It's drawn on the server
	 * whenever the page's load gives the viewer a capability on it (pageContext), fixed to the bottom of the window so it
	 * takes no room from the page, and its row is the same height in every state so nothing in it jumps.
	 *
	 * - `view`: Edit, View as visitor and the menu. Edit is a link to the page's `?edit`, so it works without scripts;
	 *   `onedit` takes it over when they run. A sign-in that ran out shows Sign in to edit instead.
	 * - `edit`: what's unsaved, Discard and Save; a draft has Save draft and Publish. The bar sits inside the page's form,
	 *   so Save and Publish submit it and Discard is a link back to the page, without scripts too. `dirty` is unknown
	 *   (undefined) until the editor runs, which keeps Save pressable for a page without scripts. On a phone the row
	 *   keeps the two buttons that move the edit on: View as visitor, and for a draft Discard, move into the menu.
	 * - `visitor`: shrinks to one pill that brings the editor back as it was.
	 *
	 * `panel` opens above the row, attached to it: the menu (`menuOpen`) or a save's conflict. `name` names the page
	 * ("Halflight"). Staff on someone else's page (`role` staff) get a Staff mark beside it, and Edit says it's as
	 * staff: opening it and every save in it go to the audit log under their role, and the bar says so the whole time.
	 */
	let {
		phase,
		activity = 'idle',
		dirty,
		status,
		missing,
		name,
		role = 'owner',
		editable = true,
		signedOut = false,
		shortcuts = false,
		error,
		editHref,
		viewHref,
		signInHref,
		menuOpen = $bindable(false),
		panel,
		editButton = $bindable(),
		onedit,
		onprefetch,
		ondiscard,
		onvisitor
	}: {
		phase: EditPhase;
		activity?: EditActivity;
		dirty?: boolean;
		status: PageStatus;
		/** What a draft still needs before it can be published, like "a name". */
		missing: string[];
		name: string;
		role?: EditRole;
		/** Whether the viewer may edit the page's content (page.edit), not only reach its menu. */
		editable?: boolean;
		signedOut?: boolean;
		/** The footer's keyboard shortcuts switch: E opens the editor only while it's on. */
		shortcuts?: boolean;
		error?: string;
		editHref: string;
		viewHref: string;
		signInHref: string;
		menuOpen?: boolean;
		panel?: Snippet;
		editButton?: HTMLAnchorElement;
		onedit?: () => void;
		/** The pointer or focus reached Edit: the editor's code can start loading. */
		onprefetch?: () => void;
		ondiscard?: () => void;
		onvisitor?: (on: boolean) => void;
	} = $props();

	const isDraft = $derived(status === 'draft');

	const asStaff = $derived(role === 'staff');

	const isBusy = $derived(activity === 'saving' || activity === 'opening');

	let menuButton = $state<HTMLButtonElement>();

	const statusLine: Record<PageStatus, string> = {
		draft: 'Draft. Only its people can see it',
		unlisted: 'Not public yet. Waiting for staff to list it',
		listed: 'Public',
		private: 'Private. Anyone with the link can see it'
	};

	/** Two lines on the left: what's going on, then what it means. Errors and conflicts lead with an icon. */
	type Summary = { title: string; detail: string; tone?: 'critical' | 'warning' };

	const summary: Summary = $derived.by(() => {
		if (activity === 'open-failed') return { title: "Couldn't open the editor", detail: 'Check your connection and try again', tone: 'critical' };

		if (phase === 'view') return { title: name, detail: signedOut ? 'Your sign-in ran out' : statusLine[status] };

		if (activity === 'saving') return { title: isDraft ? 'Saving draft' : 'Saving', detail: 'Keep this page open for a moment' };

		if (activity === 'conflict') return { title: 'Someone else saved', detail: 'Pick what to keep, then save', tone: 'warning' };

		if (activity === 'reauth') return { title: 'Sign in again to save', detail: 'Your changes stay in this browser', tone: 'warning' };

		if (activity === 'failed') return { title: "Couldn't save", detail: error ?? 'Your changes are still here. Try again', tone: 'critical' };

		if (isDraft && missing.length) return { title: 'Draft', detail: `Needs ${new Intl.ListFormat('en').format(missing)}` };

		if (isDraft) return { title: 'Draft', detail: dirty ? 'Unsaved changes' : 'Only its people see it' };

		if (asStaff) return { title: dirty ? 'Unsaved changes' : 'Editing', detail: dirty ? 'Logged as staff on save' : 'Opening this was logged' };

		return dirty ? { title: 'Unsaved changes', detail: 'Kept in this browser' } : { title: 'Editing', detail: 'Nothing changed yet' };
	});

	/** A link that scripts take over, left to the browser when they don't (or for a new tab). */
	const takeOver = (run: (() => void) | undefined) => (event: MouseEvent) => {
		if (!run || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
		event.preventDefault();
		run();
	};

	function onkeydown(event: KeyboardEvent) {
		if (event.key !== 'Escape' || !menuOpen) return;
		menuOpen = false;
		menuButton?.focus();
	}
</script>

<svelte:window {onkeydown} />

{#snippet spinner()}<LoaderCircle aria-hidden="true" class="size-4 motion-safe:animate-spin" />{/snippet}

{#snippet menuToggle()}
	<button bind:this={menuButton} type="button" class="btn-ghost btn-sm size-9 px-0" aria-expanded={menuOpen} aria-controls={menuOpen ? 'edit-bar-panel' : undefined} aria-label="More for this page" title="More" onclick={() => (menuOpen = !menuOpen)}>
		<Ellipsis aria-hidden="true" class="size-4" />
	</button>
{/snippet}

{#snippet visitorButton()}
	<button type="button" class={['btn-ghost btn-sm max-sm:size-9 max-sm:px-0', phase === 'edit' && 'max-sm:hidden']} onclick={() => onvisitor?.(true)} disabled={isBusy} title="View as visitor">
		<Eye aria-hidden="true" class="size-4" /><span class="max-sm:sr-only">View as visitor</span>
	</button>
{/snippet}

<section aria-label="Edit this page" data-edit-bar data-phase={phase} class="pointer-events-none fixed inset-x-0 bottom-0 z-40 sm:bottom-4 sm:px-4 print:hidden">
	{#if phase === 'visitor'}
		<div class="pointer-events-auto mx-auto mb-[max(1rem,env(safe-area-inset-bottom))] flex w-fit max-w-[calc(100%-2rem)] items-center gap-3 rounded-full border border-line-strong bg-raised py-1.5 pr-1.5 pl-4 text-small sm:mb-0">
			<Eye aria-hidden="true" class="size-4 shrink-0 text-fg-muted" />
			<span class="min-w-0 truncate" role="status">Viewing as a visitor</span>
			<button type="button" class="btn-secondary btn-sm rounded-full" onclick={() => onvisitor?.(false)}>Back to editing</button>
		</div>
	{:else}
		<div class="pointer-events-auto mx-auto flex max-w-3xl flex-col border-t border-line-strong bg-raised sm:rounded-xl sm:border">
			{#if panel}<div id="edit-bar-panel" class="max-h-80 overflow-auto border-b border-line">{@render panel()}</div>{/if}
			<div class="flex min-h-16 items-center gap-3 px-3 py-2.5 pb-[max(0.75rem,env(safe-area-inset-bottom))] sm:px-4 sm:pb-2.5">
				<div class="flex min-w-0 flex-1 items-start gap-2" role="status" aria-live="polite">
					{#if summary.tone}<CircleAlert aria-hidden="true" class={['mt-0.5 size-4 shrink-0', summary.tone === 'critical' ? 'text-critical' : 'text-warning']} />{/if}
					{#if phase === 'edit' && dirty && !summary.tone}<span aria-hidden="true" class="mt-2 size-1.5 shrink-0 rounded-full bg-fg-muted"></span>{/if}
					<span class="flex min-w-0 flex-col">
						<span class="flex min-w-0 items-center gap-2">
							<span class="truncate text-small font-medium text-fg">{summary.title}</span>
							{#if asStaff}<span class="pill shrink-0 gap-1 bg-warning/15 text-fg"><ShieldCheck aria-hidden="true" class="size-3 text-warning" /><span class="max-sm:sr-only">Staff</span></span>{/if}
						</span>
						<span class="truncate text-label text-fg-muted">{summary.detail}</span>
					</span>
				</div>
				<div class="flex shrink-0 items-center gap-1.5 sm:gap-2">
					{#if phase === 'view'}
						{@render menuToggle()}
						{#if signedOut}
							<a href={signInHref} class="btn-primary btn-sm">Sign in to edit</a>
						{:else if editable}
							{@render visitorButton()}
							<!-- Staff opening it is logged by the page's load, so hovering mustn't load it ahead. -->
							<a
								bind:this={editButton}
								href={editHref}
								class="btn-primary btn-sm"
								onclick={takeOver(onedit)}
								onpointerenter={onprefetch}
								onfocus={onprefetch}
								data-sveltekit-preload-data={asStaff ? 'off' : undefined}
								aria-keyshortcuts={shortcuts ? 'E' : undefined}
								aria-disabled={activity === 'opening' || undefined}
							>
								{#if activity === 'opening'}{@render spinner()}{:else}<Pencil aria-hidden="true" class="size-4" />{/if}{activity === 'open-failed' ? 'Try again' : asStaff ? 'Edit as staff' : 'Edit'}
							</a>
						{:else}
							{@render visitorButton()}
						{/if}
					{:else}
						<!-- A conflict holds the panel, and on a phone the row needs the room. -->
						{#if activity !== 'conflict'}{@render menuToggle()}{@render visitorButton()}{/if}
						{#if activity === 'reauth'}
							<a href={signInHref} class="btn-primary btn-sm">Sign in again</a>
						{:else}
							<a href={viewHref} class={['btn-ghost btn-sm', isDraft && 'max-sm:hidden', isBusy && 'pointer-events-none opacity-50']} aria-disabled={isBusy || undefined} onclick={takeOver(ondiscard)}>{dirty === false ? 'Done' : 'Discard'}</a>
							{#if isDraft}
								<button type="submit" class="btn-secondary btn-sm" disabled={isBusy || dirty === false}>Save draft</button>
								<button type="submit" name="publish" value="on" class="btn-primary btn-sm" disabled={isBusy}>
									{#if activity === 'saving'}{@render spinner()}{/if}Publish
								</button>
							{:else}
								<button type="submit" class="btn-primary btn-sm" disabled={isBusy || (dirty === false && activity === 'idle')} aria-keyshortcuts="Meta+S Control+S" aria-busy={activity === 'saving'}>
									{#if activity === 'saving'}{@render spinner()}Saving{:else}Save{/if}
								</button>
							{/if}
						{/if}
					{/if}
				</div>
			</div>
		</div>
	{/if}
</section>

<style>
	/* Editors get the bar's height kept free at the end of the page, drawn with the page, so the footer is never under it. */
	:global(html:has([data-edit-bar])) {
		--edit-bar-space: 5.5rem;
		scroll-padding-bottom: 6.5rem;
	}
	:global(html:has([data-edit-bar][data-phase='visitor'])) {
		--edit-bar-space: 4.5rem;
	}
	:global(body:has([data-edit-bar])) {
		padding-bottom: var(--edit-bar-space);
	}
</style>
