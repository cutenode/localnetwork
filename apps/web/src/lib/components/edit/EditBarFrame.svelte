<script lang="ts">
	import AddSlot from './AddSlot.svelte';
	import ConflictPanel from './ConflictPanel.svelte';
	import EditBar from './EditBar.svelte';
	import EditMenu from './EditMenu.svelte';
	import InlineText from './InlineText.svelte';
	import LeaveDialog from './LeaveDialog.svelte';
	import SaveToast from './SaveToast.svelte';
	import './inline-text.css';
	import type { ConflictField, EditActivity, EditPhase, EditRole, PageStatus } from './types.ts';

	/**
	 * Edit mode's parts drawn in one state on a small act page, for the stories: the bar and what opens from it, the
	 * toast and the leave question. EditMode moves between these states on a real page; here each is held still.
	 */
	let {
		phase = 'view',
		activity = 'idle',
		dirty,
		status = 'listed',
		missing = [],
		role = 'owner',
		signedOut = false,
		menu = false,
		leaving = false,
		toast = null,
		undo = false,
		refused,
		empty = false
	}: {
		phase?: EditPhase;
		activity?: EditActivity;
		dirty?: boolean;
		status?: PageStatus;
		missing?: string[];
		role?: EditRole;
		signedOut?: boolean;
		menu?: boolean;
		leaving?: boolean;
		toast?: string | null;
		undo?: boolean;
		/** A reason the server gave for refusing the name. */
		refused?: string;
		/** A page being created: nothing filled in yet. */
		empty?: boolean;
	} = $props();

	const name = $derived(empty ? 'New act' : 'Halflight');

	let conflict = $state<ConflictField[]>([{ name: 'tagline', label: 'Tagline', theirs: 'Deep house for small rooms, Thursdays at Oye', yours: 'Deep, patient house, mostly records', keep: 'yours' }]);

	const nothing = () => {};
</script>

{#snippet panel()}
	{#if activity === 'conflict'}
		<ConflictPanel bind:fields={conflict} />
	{:else}
		<EditMenu
			{name}
			{status}
			{role}
			links={{ people: ['Neon Tide', 'Sam', 'Ilse'], peopleHref: '#people', inboxHref: '#inbox', waiting: 2, dashboardHref: '#dashboard' }}
			auditHref="#audit"
			edit={phase === 'view' ? { href: '#edit', run: nothing } : undefined}
			onvisitor={phase === 'edit' ? nothing : undefined}
			discard={phase === 'edit' && status === 'draft' ? { label: dirty ? 'Discard' : 'Done', run: nothing } : undefined}
		/>
	{/if}
{/snippet}

<main class="container-page flex flex-col gap-4 py-8">
	{#if status === 'draft' && phase !== 'visitor'}<p class="notice notice-info">This is a draft. Only its people can see it.</p>{/if}
	{#if phase === 'edit'}
		<h1 class="text-display"><InlineText class="text-display" name="name" label="Name" required placeholder="Act name" value={empty ? '' : 'Halflight'} error={refused} readonly={activity === 'saving'} /></h1>
		<p class="text-emphasis font-normal text-fg-soft"><InlineText class="text-emphasis font-normal" name="tagline" label="Tagline" placeholder="A line about the act" value={empty ? '' : dirty ? 'Deep, patient house, mostly records' : 'Deep, patient house for small rooms'} readonly={activity === 'saving'} /></p>
		<InlineText name="bio" label="About" multiline placeholder="Who they are and how they play" value={empty ? '' : 'Halflight is Neon and Sam, playing back to back since a basement party in 2019.'} readonly={activity === 'saving'} class="text-body text-fg-soft" />
		<AddSlot label="Add links" hint="Where to hear them and book them" />
	{:else}
		<h1 class="text-display">Halflight</h1>
		<p class="text-emphasis font-normal text-fg-soft">Deep, patient house for small rooms</p>
		<p class="text-body text-fg-soft">Halflight is Neon and Sam, playing back to back since a basement party in 2019.</p>
		{#if phase === 'view'}<p class="notice notice-info">Bookings and press kits are in its dashboard.</p>{/if}
	{/if}
</main>

<EditBar
	{phase}
	{activity}
	{dirty}
	{status}
	{missing}
	{name}
	{role}
	{signedOut}
	shortcuts
	error={refused}
	editHref="#edit"
	viewHref="#view"
	signInHref="#sign-in"
	menuOpen={menu}
	panel={menu || activity === 'conflict' ? panel : undefined}
	onvisitor={nothing}
/>

<SaveToast message={toast} undo={undo ? nothing : undefined} onclose={nothing} />

<LeaveDialog open={leaving} {name} onstay={nothing} ondiscard={nothing} onsave={nothing} />
