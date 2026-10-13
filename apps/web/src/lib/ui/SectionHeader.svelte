<script lang="ts">
	import type { Snippet } from 'svelte';
	import Eyebrow, { type Tone } from './Eyebrow.svelte';

	/**
	 * A section's eyebrow, title and lede. `start` stacks them on the left, `center` is for heroes
	 * and closing calls, `split` puts the lede beside the title. `display` is the hero size.
	 */
	let {
		id,
		eyebrow,
		tone,
		title,
		lede,
		level = 2,
		size = 'title',
		align = 'start',
		actions
	}: {
		id?: string;
		eyebrow?: string;
		tone?: Tone;
		title: string;
		lede?: string;
		level?: 1 | 2 | 3;
		size?: 'title' | 'display';
		align?: 'start' | 'center' | 'split';
		actions?: Snippet;
	} = $props();

	const display = $derived(size === 'display');
</script>

<div class={['flex gap-x-14 gap-y-4.5', align === 'split' ? 'flex-wrap items-end justify-between' : 'flex-col', align === 'center' && 'mx-auto max-w-245 items-center text-center', align === 'start' && 'max-w-190']}>
	<div class={['flex flex-col gap-4.5', align === 'split' && 'min-w-0 flex-[1_1_32rem]', align === 'center' && 'items-center']}>
		{#if eyebrow}<Eyebrow {tone}>{eyebrow}</Eyebrow>{/if}
		<svelte:element this={`h${level}`} {id} class={['m-0 font-semibold text-balance', display ? 'text-display' : 'text-title']}>{title}</svelte:element>
	</div>
	{#if lede}
		<p class={['m-0 text-fg-muted', display ? 'max-w-160 text-intro' : 'text-lede', align === 'split' && 'min-w-0 flex-[1_1_22rem]']}>{lede}</p>
	{/if}
	{#if actions}<div class={['flex flex-wrap gap-3', align === 'center' && 'justify-center']}>{@render actions()}</div>{/if}
</div>
