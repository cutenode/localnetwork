<script module lang="ts">
	import type { LucideIcon } from '@lucide/svelte';

	/**
	 * `badge` is a count waiting there, `dot` says only that something's new, and `exact` keeps an overview from being
	 * current on the pages under it. An item is marked with an icon, or with `monogram`, a name to take initials from,
	 * when it's one of someone's own things.
	 */
	export type SidebarItem = { href: string; label: string; badge?: number; dot?: boolean; exact?: boolean } & ({ icon: LucideIcon; monogram?: undefined } | { monogram: string; icon?: undefined });

	/**
	 * Items that come from a slower lookup arrive as a promise, and show as placeholders until then. `continues` sits a
	 * group's items straight under the group before it, and shows no placeholders, for items that may not be there.
	 */
	export type SidebarGroup = { label?: string; continues?: boolean; items: SidebarItem[] | Promise<SidebarItem[]> };
</script>

<script lang="ts">
	import { ChevronDown, Menu, X } from '@lucide/svelte';
	import type { Snippet } from 'svelte';
	import Eyebrow from './Eyebrow.svelte';
	import Monogram from './Monogram.svelte';

	/**
	 * A signed-in area's page: a sidebar of `groups`, then `footer` below a rule, beside the page. On a
	 * phone the sidebar folds into a menu button that names the current section with its icon, so it
	 * doesn't read as a second site menu under the header's. With no current section it falls back to
	 * the header's own menu icon, which keeps that icon in the chunk every page shares. `current` is the
	 * page's path; an item is current on its own page and the pages under it.
	 *
	 * With `tabs`, a phone gets those items as a bar at the bottom instead of the menu button, and a last
	 * tab opens everything else in a sheet, so destinations stay named rather than folded away. `title`
	 * labels the sidebar; without it the sidebar opens straight on its items.
	 */
	let {
		label,
		title,
		groups,
		footer = [],
		tabs,
		current,
		open = $bindable(false),
		children
	}: { label: string; title?: string; groups: SidebarGroup[]; footer?: SidebarItem[]; tabs?: SidebarItem[]; current: string; open?: boolean; children: Snippet } = $props();

	const id = $props.id();

	const isCurrent = (item: SidebarItem) => current === item.href || (!item.exact && current.startsWith(`${item.href}/`));

	// Items from a slower lookup, once they arrive, so the menu button can name a page among them too.
	let arrived = $state<SidebarItem[]>([]);

	$effect(() => {
		let live = true;
		void Promise.all(groups.map((group) => group.items)).then((items) => {
			if (live) arrived = items.flat();
		});

		return () => (live = false);
	});

	// The most specific current item, so a project's page under Projects doesn't read as the overview.
	const here = $derived(
		[...groups.flatMap((group) => (Array.isArray(group.items) ? group.items : [])), ...arrived, ...footer]
			.filter(isCurrent)
			.sort((a, b) => b.href.length - a.href.length)[0]
	);

	let sheet = $state<HTMLDialogElement>();

	$effect(() => {
		if (open && !sheet?.open) sheet?.showModal();
		else if (!open) sheet?.close();
	});
</script>

{#snippet mark(item: SidebarItem)}
	{#if item.icon}<item.icon class="size-4 shrink-0" aria-hidden="true" />{:else}<Monogram name={item.monogram} size="xs" />{/if}
{/snippet}

{#snippet link(item: SidebarItem)}
	{@const active = isCurrent(item)}
	<li>
		<a href={item.href} aria-current={active ? 'page' : undefined} onclick={() => (open = false)} class={['flex min-h-10 items-center gap-2.5 rounded-md px-3 text-small no-underline transition-colors', active ? 'bg-raised font-medium text-fg' : 'text-fg-muted hover:bg-raised hover:text-fg']}>
			{@render mark(item)}<span class="min-w-0 truncate">{item.label}</span>
			{#if item.badge}<span class="ml-auto rounded-full bg-fg px-1.5 font-mono text-caption font-semibold text-fg-inverse">{item.badge}</span>{:else if item.dot}<span class="ml-auto size-2 rounded-full bg-link"><span class="sr-only">Unread</span></span>{/if}
		</a>
	</li>
{/snippet}

{#snippet heading(group: SidebarGroup, headingId: string)}
	{#if group.label}<p id={headingId} class="m-0 px-3 eyebrow text-fg-subtle">{group.label}</p>{/if}
{/snippet}

<!-- The sidebar's contents, `place` keeping the sheet's ids apart from the sidebar's. -->
{#snippet sections(place: string)}
	{#if title}<div class="hidden px-3 lg:block"><Eyebrow>{title}</Eyebrow></div>{/if}
	{#each groups as group, index (index)}
		{@const headingId = `${id}-${place}-group-${index}`}
		<!-- A group whose items turn out to be none leaves nothing behind, its label included. -->
		<div class={['flex flex-col gap-1.5 empty:hidden', group.continues && '-mt-4']}>
			{#await group.items}
				{#if !group.continues}
					{@render heading(group, headingId)}
					<ul aria-labelledby={group.label ? headingId : undefined} aria-busy="true" class="m-0 flex list-none flex-col gap-0.5 p-0">
						{#each { length: 2 }, row (row)}<li class="flex min-h-10 items-center px-3"><span class="h-4 w-3/4 animate-pulse rounded-md bg-raised"></span></li>{/each}
					</ul>
				{/if}
			{:then items}
				{#if items.length}
					{@render heading(group, headingId)}
					<ul aria-labelledby={group.label ? headingId : undefined} class="m-0 flex list-none flex-col gap-0.5 p-0">
						{#each items as item (item.href)}{@render link(item)}{/each}
					</ul>
				{/if}
			{/await}
		</div>
	{/each}
	{#if footer.length}
		<ul class="m-0 flex list-none flex-col gap-0.5 border-t border-line p-0 pt-3">
			{#each footer as item (item.href)}{@render link(item)}{/each}
		</ul>
	{/if}
{/snippet}

<div class={['container-page grid grid-cols-[minmax(0,1fr)] gap-6 py-6 sm:py-10 lg:grid-cols-[13rem_minmax(0,1fr)] lg:gap-10']}>
	<nav aria-label={label} class={tabs ? 'hidden lg:sticky lg:top-6 lg:block lg:self-start' : 'max-lg:border-b max-lg:border-line max-lg:pb-3 lg:sticky lg:top-6 lg:self-start'}>
		{#if tabs}
			<div class="flex flex-col gap-5">{@render sections('side')}</div>
		{:else}
			<button type="button" aria-expanded={open} aria-controls="{id}-items" aria-label="{title} menu: {here?.label ?? title}" onclick={() => (open = !open)} class="flex min-h-11 w-full items-center gap-2.5 rounded-md px-3 text-small font-medium text-fg hover:bg-raised lg:hidden">
				{#if here}{@render mark(here)}{:else}<Menu class="size-4 shrink-0" aria-hidden="true" />{/if}<span class="min-w-0 truncate">{here?.label ?? title}</span>
				<ChevronDown class={['ml-auto size-4 shrink-0 transition-transform', open && 'rotate-180']} aria-hidden="true" />
			</button>
			<div id="{id}-items" class={['flex-col gap-5 max-lg:pt-3 lg:flex', open ? 'flex' : 'hidden']}>{@render sections('side')}</div>
		{/if}
	</nav>
	<div class="min-w-0">{@render children()}</div>
</div>

{#if tabs}
	{@const onTab = tabs.some(isCurrent)}
	<nav aria-label="{label} tabs" class="fixed inset-x-0 bottom-0 z-20 grid auto-cols-fr grid-flow-col border-t border-line bg-canvas pb-[env(safe-area-inset-bottom)] lg:hidden print:hidden">
		{#each tabs as item (item.href)}
			{@const active = isCurrent(item)}
			<a href={item.href} aria-current={active ? 'page' : undefined} class={['relative flex min-h-14 flex-col items-center justify-center gap-0.5 text-caption font-medium no-underline', active ? 'text-fg' : 'text-fg-muted']}>
				{@render mark(item)}{item.label}
				{#if item.badge}<span class="absolute top-1.5 left-[calc(50%+0.375rem)] rounded-full bg-fg px-1 font-mono text-[0.625rem] leading-tight font-semibold text-fg-inverse">{item.badge}</span>{:else if item.dot}<span class="absolute top-1.5 left-[calc(50%+0.375rem)] size-2 rounded-full bg-link"><span class="sr-only">Unread</span></span>{/if}
			</a>
		{/each}
		<button type="button" aria-haspopup="dialog" aria-expanded={open} aria-label="{label} menu" onclick={() => (open = true)} class={['flex min-h-14 flex-col items-center justify-center gap-0.5 text-caption font-medium', onTab ? 'text-fg-muted' : 'text-fg']}>
			<Menu class="size-4 shrink-0" aria-hidden="true" />Menu
		</button>
	</nav>
	<!-- A click lands on the dialog itself only outside the panel, on the backdrop. -->
	<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
	<dialog bind:this={sheet} aria-label="{label} menu" onclose={() => (open = false)} onclick={(event) => event.target === sheet && (open = false)} class="m-0 mt-auto max-h-[85dvh] w-full max-w-none overflow-auto overscroll-contain rounded-t-xl border-0 border-t border-line-strong bg-surface p-0 text-fg starting:translate-y-6 transition-transform duration-200 ease-out motion-reduce:transition-none backdrop:bg-canvas/80 lg:hidden">
		<div class="flex flex-col gap-5 px-3 pt-2 pb-[max(0.75rem,env(safe-area-inset-bottom))]">
			<div class="flex justify-end">
				<button type="button" aria-label="Close menu" onclick={() => (open = false)} class="grid size-11 place-items-center rounded-md text-fg-muted hover:bg-raised hover:text-fg"><X class="size-5" aria-hidden="true" /></button>
			</div>
			{#if open}{@render sections('sheet')}{/if}
		</div>
	</dialog>
{/if}

