<script lang="ts">
	import type { NeedsRow } from '#lib/home.ts';
	import Button from '#lib/ui/Button.svelte';
	import Monogram from '#lib/ui/Monogram.svelte';

	/**
	 * What's waiting on someone, most pressing first. The first row is the page's one primary action unless `lead` is
	 * off. Each row's button stretches over the whole row, so there's one link to tab through and all of it clicks.
	 * `loading` holds the room of that many rows.
	 */
	let { rows = [], lead = true, loading = 0 }: { rows?: NeedsRow[]; lead?: boolean; loading?: number } = $props();
</script>

<ul class="card m-0 list-none divide-y divide-line overflow-hidden p-0" aria-busy={loading > 0 || undefined}>
	{#each { length: loading }, row (row)}
		{@const leads = lead && row === 0}
		<li class={['flex items-center gap-3 px-4 sm:gap-4 sm:px-5', leads ? 'py-4 sm:py-5' : 'py-3 sm:py-3.5']}>
			<span class={['shrink-0 bg-well motion-safe:animate-pulse', leads ? 'size-10 rounded-lg' : 'size-8 rounded-[9px]']}></span>
			<span class="flex flex-1 flex-col gap-2"><span class="h-4 w-1/2 rounded-md bg-well motion-safe:animate-pulse"></span><span class="h-3 w-3/5 rounded-md bg-well motion-safe:animate-pulse"></span></span>
		</li>
	{/each}
	{#each rows as row, index (row.key)}
		{@const leads = lead && index === 0}
		<li class={['relative flex items-center gap-3 px-4 transition-colors hover:bg-raised sm:gap-4 sm:px-5', leads ? 'py-4 sm:py-5' : 'py-3 sm:py-3.5']}>
			<Monogram name={row.scope} size={leads ? 'md' : 'sm'} />
			<div class="flex min-w-0 flex-1 flex-col gap-0.5">
				<p class={['m-0 text-fg', leads ? 'text-emphasis font-semibold' : 'text-small font-medium']}>{row.title}</p>
				<p class="m-0 text-label text-fg-muted">{row.detail}</p>
			</div>
			<Button href={row.href} size="sm" variant={leads ? 'primary' : 'secondary'} aria-label="{row.action}: {row.title}" class="shrink-0 after:absolute after:inset-0">{row.action}</Button>
		</li>
	{/each}
</ul>
