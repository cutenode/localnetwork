<script lang="ts">
	import { ArrowUpRight, Eye, Inbox, Pencil, ShieldCheck, Undo2 } from '@lucide/svelte';
	import type { Snippet } from 'svelte';
	import Monogram from '#lib/ui/Monogram.svelte';
	import type { EditMenuLinks, EditRole, PageStatus } from './types.ts';

	/**
	 * What the edit bar's menu holds beyond editing the page's text. The page's own part, for its people: who it shows
	 * to, who its people are, what's waiting in the inbox about it and the way to its dashboard page, each when the page
	 * has one (`links`). Then, for staff only, a marked part whose every step is logged as staff: Edit as staff, and
	 * the audit log filtered to this page. Staff on someone else's page see only the staff part.
	 */
	let {
		name,
		status,
		role = 'owner',
		links = {},
		auditHref,
		edit,
		onvisitor,
		discard
	}: {
		name: string;
		status: PageStatus;
		role?: EditRole;
		links?: EditMenuLinks;
		/** The staff console's audit log, filtered to this page. */
		auditHref: string;
		/** Given in the view phase: the same Edit (as staff) the bar has, its link and what scripts do instead. */
		edit?: { href: string; run: () => void };
		/** Given while editing: on a phone the bar has no room for View as visitor, so it's here instead. */
		onvisitor?: () => void;
		/** The same for a draft's Discard (or Done), which a phone's bar has no room for beside Save draft and Publish. */
		discard?: { label: string; run: () => void };
	} = $props();

	const isOwner = $derived(role !== 'staff');

	const isStaff = $derived(role !== 'owner');

	const standing: Record<PageStatus, string> = {
		draft: 'Only its people, until you publish',
		unlisted: 'Its people and staff, until staff list it',
		listed: 'Everyone'
	};

	const people = $derived(links.people ?? []);

	const peopleLine = $derived(people.length > 2 ? `${people.slice(0, 2).join(', ')} and ${people.length - 2} more` : people.join(' and '));
</script>

{#snippet row(title: string, body: Snippet)}
	<div class="flex min-h-11 flex-wrap items-center justify-between gap-x-4 gap-y-1.5 py-1.5">
		<span class="text-label text-fg-muted">{title}</span>
		<span class="flex min-w-0 items-center gap-2 text-small">{@render body()}</span>
	</div>
{/snippet}

<div class="flex flex-col">
	{#if onvisitor}
		<button type="button" class="flex min-h-11 items-center gap-2 border-b border-line px-4 text-left text-small text-fg hover:bg-well sm:hidden" onclick={onvisitor}><Eye aria-hidden="true" class="size-4 text-fg-muted" />View as visitor</button>
	{/if}
	{#if discard}
		<button type="button" class="flex min-h-11 items-center gap-2 border-b border-line px-4 text-left text-small text-fg hover:bg-well sm:hidden" onclick={discard.run}><Undo2 aria-hidden="true" class="size-4 text-fg-muted" />{discard.label}</button>
	{/if}
	{#if isOwner}
		<div class="flex flex-col px-4 py-3">
			<h2 class="m-0 mb-1 eyebrow text-fg-muted">{name}</h2>
			{#snippet standingBody()}<span class="text-fg">{standing[status]}</span>{/snippet}
			{@render row('Shows to', standingBody)}
			{#if people.length}
				{#snippet peopleBody()}
					<span class="faces flex" aria-hidden="true">{#each people.slice(0, 3) as person (person)}<span class="rounded-lg"><Monogram name={person} size="sm" /></span>{/each}</span>
					<span class="truncate text-fg">{peopleLine}</span>
					{#if links.peopleHref}<a href={links.peopleHref} class="link shrink-0">Manage</a>{/if}
				{/snippet}
				{@render row('People', peopleBody)}
			{/if}
			{#if links.inboxHref}
				{#snippet inboxBody()}
					<Inbox aria-hidden="true" class="size-4 text-fg-muted" />
					<a href={links.inboxHref} class="link">{links.waiting ? `${links.waiting} waiting about this page` : 'Nothing waiting'}</a>
				{/snippet}
				{@render row('Inbox', inboxBody)}
			{/if}
			{#if links.dashboardHref}
				{#snippet dashboardBody()}
					<a href={links.dashboardHref} class="link inline-flex items-center gap-1">Bookings and settings<ArrowUpRight aria-hidden="true" class="size-3.5" /></a>
				{/snippet}
				{@render row('Dashboard', dashboardBody)}
			{/if}
		</div>
	{/if}
	{#if isStaff}
		<div class={['staff flex flex-col px-4 py-3', isOwner && 'border-t']}>
			<div class="mb-1 flex flex-wrap items-baseline justify-between gap-x-3">
				<h2 class="m-0 flex items-center gap-1.5 eyebrow text-fg"><ShieldCheck aria-hidden="true" class="size-3.5 text-warning" />Staff</h2>
				<span class="text-caption text-fg-muted">Everything here is logged as staff</span>
			</div>
			{#if edit && !isOwner}
				{#snippet editBody()}
					<a
						href={edit.href}
						class="link inline-flex items-center gap-1"
						data-sveltekit-preload-data="off"
						onclick={(event) => {
							event.preventDefault();
							edit.run();
						}}><Pencil aria-hidden="true" class="size-3.5" />Edit as staff</a
					>
				{/snippet}
				{@render row('Page', editBody)}
			{/if}
			{#snippet recordBody()}<a href={auditHref} class="link">This page's audit log</a>{/snippet}
			{@render row('Record', recordBody)}
		</div>
	{/if}
</div>

<style>
	/* Its own styles rather than utilities only it would use, so they load with the editor and not in every page's. */
	.faces > * {
		box-shadow: 0 0 0 2px var(--color-raised);
	}
	.faces > * + * {
		margin-left: calc(var(--spacing) * -1.5);
	}
	.staff {
		background: color-mix(in oklab, var(--color-warning) 5%, transparent);
		border-color: color-mix(in oklab, var(--color-warning) 30%, transparent);
	}
</style>
