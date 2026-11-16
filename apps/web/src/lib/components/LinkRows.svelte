<script lang="ts">
	import type { LucideIcon } from '@lucide/svelte';
	import { ChevronRight } from '@lucide/svelte';

	/**
	 * One row. `detail` is its line and `count` its number, null while they're on their way. Without `href` it's a plain
	 * row, for something there's nothing to open on.
	 */
	type LinkRow = { label: string; detail: string | null; href?: string; icon?: LucideIcon; count?: number | null };

	/** Places to go, each with a line saying where things stand there. Each row's link stretches over all of it. */
	let { rows }: { rows: LinkRow[] } = $props();
</script>

<ul class="card m-0 list-none divide-y divide-line overflow-hidden p-0">
	{#each rows as row (row.label)}
		<li class={['relative flex items-center gap-3 px-4 py-3 sm:gap-4 sm:px-5 sm:py-3.5', row.href && 'transition-colors hover:bg-raised']}>
			{#if row.icon}<span class="grid size-8 shrink-0 place-items-center rounded-[9px] bg-well text-fg-muted"><row.icon class="size-4" aria-hidden="true" /></span>{/if}
			<div class="flex min-w-0 flex-1 flex-col gap-0.5">
				{#if row.href}
					<a href={row.href} class="text-small font-medium text-fg no-underline after:absolute after:inset-0">{row.label}</a>
				{:else}
					<p class="m-0 text-small font-medium text-fg-muted">{row.label}</p>
				{/if}
				<!-- Always a line, so nothing moves when it arrives. -->
				<p class="m-0 flex min-h-lh items-center text-label text-fg-muted">
					{#if row.detail === null}<span class="h-3 w-2/5 rounded-md bg-well motion-safe:animate-pulse"></span>{:else}<span>{row.detail}</span>{/if}
				</p>
			</div>
			{#if row.count === null}
				<span class="h-4 w-5 shrink-0 rounded-md bg-well motion-safe:animate-pulse"></span>
			{:else if row.count !== undefined}
				<span class={['shrink-0 font-mono text-small', row.count ? 'text-fg' : 'text-fg-subtle']}>{row.count}</span>
			{/if}
			<!-- Takes its room either way, so counts line up down the list and don't move when a row becomes a link. -->
			<ChevronRight class={['size-4 shrink-0 text-fg-subtle', !row.href && 'invisible']} aria-hidden="true" />
		</li>
	{/each}
</ul>
