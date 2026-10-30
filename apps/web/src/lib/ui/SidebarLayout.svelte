<script module lang="ts">
	import type { LucideIcon } from '@lucide/svelte';

	/** `badge` is a count waiting there; `exact` keeps an overview from being current on the pages under it. */
	export type SidebarItem = { href: string; label: string; icon: LucideIcon; badge?: number; exact?: boolean };

	/** Items that come from a slower lookup arrive as a promise, and show as placeholders until then. */
	export type SidebarGroup = { label?: string; items: SidebarItem[] | Promise<SidebarItem[]> };
</script>

<script lang="ts">
	import { ChevronDown, Menu } from '@lucide/svelte';
	import type { Snippet } from 'svelte';
	import Eyebrow from './Eyebrow.svelte';

	/**
	 * A signed-in area's page: a sidebar of `groups`, then `footer` below a rule, beside the page. On a
	 * phone the sidebar folds into a menu button that names the current section. `current` is the
	 * page's path; an item is current on its own page and the pages under it.
	 */
	let {
		label,
		title,
		groups,
		footer = [],
		current,
		open = $bindable(false),
		children
	}: { label: string; title: string; groups: SidebarGroup[]; footer?: SidebarItem[]; current: string; open?: boolean; children: Snippet } = $props();

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
</script>

{#snippet link(item: SidebarItem)}
	{@const active = isCurrent(item)}
	<li>
		<a href={item.href} aria-current={active ? 'page' : undefined} onclick={() => (open = false)} class={['flex min-h-10 items-center gap-2.5 rounded-md px-3 text-small no-underline transition-colors', active ? 'bg-raised font-medium text-fg' : 'text-fg-muted hover:bg-raised hover:text-fg']}>
			<item.icon class="size-4 shrink-0" aria-hidden="true" /><span class="min-w-0 truncate">{item.label}</span>
			{#if item.badge}<span class="ml-auto rounded-full bg-fg px-1.5 font-mono text-caption font-semibold text-fg-inverse">{item.badge}</span>{/if}
		</a>
	</li>
{/snippet}

<div class="container-page grid grid-cols-[minmax(0,1fr)] gap-6 py-6 sm:py-10 lg:grid-cols-[13rem_minmax(0,1fr)] lg:gap-10">
	{#snippet heading(group: SidebarGroup, index: number)}
	{#if group.label}<p id="{id}-group-{index}" class="m-0 px-3 eyebrow text-fg-subtle">{group.label}</p>{/if}
{/snippet}

<nav aria-label={label} class="max-lg:border-b max-lg:border-line max-lg:pb-3 lg:sticky lg:top-6 lg:self-start">
		<button type="button" aria-expanded={open} aria-controls="{id}-items" aria-label="{title} menu: {here?.label ?? title}" onclick={() => (open = !open)} class="flex min-h-11 w-full items-center gap-2.5 rounded-md px-3 text-small font-medium text-fg hover:bg-raised lg:hidden">
			<Menu class="size-4 shrink-0" aria-hidden="true" /><span class="min-w-0 truncate">{here?.label ?? title}</span>
			<ChevronDown class={['ml-auto size-4 shrink-0 transition-transform', open && 'rotate-180']} aria-hidden="true" />
		</button>
		<div id="{id}-items" class={['flex-col gap-5 max-lg:pt-3 lg:flex', open ? 'flex' : 'hidden']}>
			<div class="hidden px-3 lg:block"><Eyebrow>{title}</Eyebrow></div>
			{#each groups as group, index (index)}
				<!-- A group whose items turn out to be none leaves nothing behind, its label included. -->
				<div class="flex flex-col gap-1.5 empty:hidden">
					{#await group.items}
						{@render heading(group, index)}
						<ul aria-labelledby={group.label ? `${id}-group-${index}` : undefined} aria-busy="true" class="m-0 flex list-none flex-col gap-0.5 p-0">
							{#each { length: 2 }, row (row)}<li class="flex min-h-10 items-center px-3"><span class="h-4 w-3/4 animate-pulse rounded-md bg-raised"></span></li>{/each}
						</ul>
					{:then items}
						{#if items.length}
							{@render heading(group, index)}
							<ul aria-labelledby={group.label ? `${id}-group-${index}` : undefined} class="m-0 flex list-none flex-col gap-0.5 p-0">
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
		</div>
	</nav>
	<div class="min-w-0">{@render children()}</div>
</div>
