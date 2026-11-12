<script lang="ts">
	import { CircleAlert } from '@lucide/svelte';
	import type { NightRow } from '#lib/home.ts';
	import DateBlock from '#lib/ui/DateBlock.svelte';

	/**
	 * Nights in date order, each led by its date, dashed while it's not settled, and flagged when inquiries clash on it.
	 * `loading` holds their room.
	 */
	let { nights = [], loading = false }: { nights?: NightRow[]; loading?: boolean } = $props();
</script>

<ul class={['card m-0 list-none divide-y divide-line overflow-hidden p-0', !loading && 'list-in']} aria-busy={loading || undefined}>
	{#if loading}
		{#each { length: 3 }, row (row)}
			<li class="flex items-center gap-3 px-4 py-2.5 sm:gap-4 sm:px-5">
				<span class="h-[4.375rem] w-12 shrink-0 rounded-lg bg-well motion-safe:animate-pulse"></span>
				<span class="flex flex-1 flex-col gap-2"><span class="h-4 w-2/5 rounded-md bg-well motion-safe:animate-pulse"></span><span class="h-3 w-3/5 rounded-md bg-well motion-safe:animate-pulse"></span></span>
			</li>
		{/each}
	{/if}
	{#each nights as night (night.key)}
		<li class="relative flex items-center gap-3 px-4 py-2.5 transition-colors hover:bg-raised sm:gap-4 sm:px-5">
			<DateBlock date={night.date} tentative={night.tentative} compact />
			<div class="flex min-w-0 flex-1 flex-col gap-0.5">
				<a href={night.href} class="text-small font-medium text-fg no-underline after:absolute after:inset-0">{night.title}</a>
				<p class="m-0 text-label text-fg-muted">{night.detail}</p>
				{#if night.inquiries}<p class="m-0 inline-flex items-center gap-1.5 text-label text-fg-soft"><CircleAlert class="size-3.5 shrink-0 text-warning" aria-hidden="true" />{night.inquiries} inquiries on this night</p>{/if}
			</div>
			{#if night.when}<span class="pill shrink-0 bg-well text-fg-soft">{night.when}</span>{/if}
		</li>
	{/each}
</ul>

<style>
	/* Results fade over the placeholders they replace, in the same room, so nothing moves. */
	@media (prefers-reduced-motion: no-preference) {
		.list-in {
			animation: list-in 180ms ease-out;
		}
	}

	@keyframes list-in {
		from {
			opacity: 0;
		}
	}
</style>
