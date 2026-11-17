<script lang="ts">
	import { type ComponentProps, type Snippet, tick, untrack } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import type { Capability } from '#lib/capabilities.ts';
	import { draftKey } from '#lib/form-draft.svelte.ts';
	import { hotkeys, hotkeyTarget } from '#lib/hotkeys.svelte.ts';
	import EditBar from './EditBar.svelte';
	import { editUrl } from './edit-url.ts';
	import type { FieldEntries } from './fields.ts';
	import type { ConflictField, EditActivity, EditMenuLinks, EditPhase, EditRefusal, EditRole, EditSubject, EditView, Navigate, PageStatus } from './types.ts';

	/**
	 * Edit mode around a page that shows one thing. For a visitor it draws the page and nothing more. For someone the
	 * page's load gave a capability (pageContext), the server also draws the edit bar, so nothing shifts as the page
	 * hydrates; with `editing` (openEditor, from `?edit`) the page's content draws its fields inside one form that posts
	 * to the page's `?/save`, which works without scripts. The page draws its content in `children`, from the EditView
	 * it's handed: fields while editing, what a visitor sees while viewing as one.
	 *
	 * Everything that only matters once someone edits (the editor's session, the menu, the toast, a conflict's choices)
	 * loads with `import()` when Edit is pressed or hovered, or the menu opens, so nobody else downloads it.
	 */
	let {
		subject,
		can = [],
		role,
		editing = false,
		name,
		status,
		missing = [],
		version,
		labels,
		form,
		links,
		signedOut = false,
		url,
		viewer,
		navigate = goto,
		guardNavigation,
		enhance,
		importEditor = () => import('./editor.ts'),
		children
	}: {
		subject?: EditSubject;
		can?: Capability[];
		role?: EditRole;
		editing?: boolean;
		/** The page's name, for the bar ("Halflight"). */
		name: string;
		status: PageStatus;
		missing?: string[];
		/** The version (CID) of the page the form opens on, for the stale-write check. */
		version: string;
		/** Each field's label, by name, for a conflict's choices. */
		labels: Record<string, string>;
		/** The page's action data: a save refused without scripts. */
		form?: EditRefusal | null;
		links?: EditMenuLinks;
		signedOut?: boolean;
		/** The page's address and who's signed in: SvelteKit's page state's, unless a harness gives its own. */
		url?: { pathname: string; search: string };
		viewer?: { did: string } | null;
		navigate?: Navigate;
		/** Where the leave question's guard goes: the app's navigations, unless a harness drives its own. */
		guardNavigation?: ComponentProps<Editor['EditSession']>['guardNavigation'];
		/** How the form is sent: SvelteKit's enhance, unless a harness answers for the server. */
		enhance?: ComponentProps<Editor['EditSession']>['enhance'];
		/** Where the editor's code comes from: its own chunk, unless a harness has it fail to arrive. */
		importEditor?: () => Promise<Editor>;
		children: Snippet<[EditView]>;
	} = $props();

	type Editor = typeof import('./editor.ts');

	let editor = $state<Editor>();

	let loading: Promise<Editor> | undefined;

	let session = $state<{ discard: () => Promise<void> }>();

	let formElement = $state<HTMLFormElement | null>(null);

	let editButton = $state<HTMLAnchorElement>();

	// A save refused without scripts starts the page in that state; after that, the editor keeps it.
	let activity = $state<EditActivity>(untrack(() => (form?.reauth ? 'reauth' : form?.error ? 'failed' : 'idle')));

	let dirty = $state<boolean>();

	let menuOpen = $state(false);

	let visitor = $state(false);

	let conflict = $state<ConflictField[]>([]);

	let refusal = $state<EditRefusal | null>(null);

	let restore = $state<FieldEntries>();

	let toast = $state<{ message: string; undo?: () => void } | null>(null);

	let toastTimer: ReturnType<typeof setTimeout> | undefined;

	const editable = $derived(can.includes('page.edit'));

	const phase: EditPhase = $derived(visitor ? 'visitor' : editing ? 'edit' : 'view');

	const here = $derived(url ?? page.url);

	const editHref = $derived(editUrl(here, true));

	const viewHref = $derived(editUrl(here, false));

	const shown = $derived(refusal ?? form ?? null);

	const view: EditView = $derived({
		editing: phase === 'edit',
		visitor,
		busy: activity === 'saving',
		value: (field, saved) => shown?.values?.[field] ?? saved,
		error: (field) => (shown?.field?.name === field ? shown.error : undefined)
	});

	const loadEditor = () =>
		(loading ??= importEditor().then(
			(loaded) => {
				editor = loaded;

				// It's here now, so an earlier failure to load it no longer stands.
				if (activity === 'open-failed') activity = 'idle';

				return loaded;
			},
			(cause: unknown) => {
				loading = undefined;
				throw cause;
			}
		));

	const prefetch = () => void loadEditor().catch(() => {});

	/** Opens the editor: its code and the page's `?edit`, together. Staff opening it are logged by that load. */
	async function open() {
		if (activity === 'opening') return;
		activity = 'opening';
		menuOpen = false;

		try {
			await Promise.all([loadEditor(), navigate(editHref, { reset: false })]);
			activity = 'idle';
		} catch {
			activity = 'open-failed';
		}
	}

	/** Back to the view from a save or a discard: says how it went, and gives focus back to Edit. */
	async function finish(message: string | null, typed?: FieldEntries) {
		restore = undefined;
		clearTimeout(toastTimer);
		toast = message ? { message, undo: typed && (() => undo(typed)) } : null;

		if (message) toastTimer = setTimeout(() => (toast = null), typed ? 8000 : 4000);
		await tick();
		editButton?.focus();
	}

	function undo(typed: FieldEntries) {
		toast = null;
		restore = typed;
		void open();
	}

	function onvisitor(on: boolean) {
		visitor = on;
		menuOpen = false;
	}

	// A page opened at `?edit` (a link, a reload) loads the editor at once; so does opening the menu.
	$effect(() => {
		// Read first, so opening the menu tries again even in edit mode, after the editor failed to load.
		const asked = menuOpen;

		if ((editing || asked) && role) void loadEditor().catch(() => (activity = 'open-failed'));
	});

	function onkeydown(event: KeyboardEvent) {
		if (phase !== 'view' || !editable || signedOut || !hotkeyTarget(event, { e: editHref })) return;
		event.preventDefault();
		void open();
	}

	const auditHref = $derived(subject ? `/admin/audit?q=${encodeURIComponent(subject.uri)}` : '');
</script>

<svelte:window {onkeydown} />

{#snippet panel()}
	{#if editor && activity === 'conflict'}
		<editor.ConflictPanel bind:fields={conflict} />
	{:else if editor && menuOpen}
		<editor.EditMenu
			{name}
			{status}
			{role}
			{links}
			{auditHref}
			edit={phase === 'view' ? { href: editHref, run: open } : undefined}
			onvisitor={phase === 'edit' ? () => onvisitor(true) : undefined}
			discard={phase === 'edit' && status === 'draft' && session ? { label: dirty ? 'Discard' : 'Done', run: session.discard } : undefined}
		/>
	{/if}
{/snippet}

{#snippet bar()}
	<EditBar
		{phase}
		{activity}
		{dirty}
		{status}
		{missing}
		{name}
		{role}
		{editable}
		{signedOut}
		shortcuts={hotkeys.enabled}
		error={shown?.error}
		{editHref}
		{viewHref}
		signInHref={`/login?next=${encodeURIComponent(editHref)}`}
		bind:menuOpen
		bind:editButton
		panel={editor && (menuOpen || activity === 'conflict') ? panel : undefined}
		onedit={open}
		onprefetch={prefetch}
		ondiscard={session?.discard}
		{onvisitor}
	/>
{/snippet}

{#if !role || !subject}
	{@render children(view)}
{:else if phase === 'edit'}
	<form bind:this={formElement} method="POST" action="?/save" class="contents">
		<input type="hidden" name="cid" value={version} />
		{@render children(view)}
		{@render bar()}
	</form>
	{#if editor && formElement}
		<editor.EditSession
			bind:this={session}
			form={formElement}
			{name}
			{status}
			{labels}
			draftKey={draftKey(viewer === undefined ? page.data.viewer : viewer, `edit:${subject.uri}`)}
			{restore}
			{viewHref}
			bind:activity
			bind:dirty
			bind:conflict
			bind:refusal
			{navigate}
			{guardNavigation}
			{enhance}
			onfinish={finish}
		/>
	{/if}
{:else}
	{@render children(view)}
	{@render bar()}
{/if}

{#if editor && role}<editor.SaveToast message={toast?.message ?? null} undo={toast?.undo} onclose={() => (toast = null)} />{/if}
