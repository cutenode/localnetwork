<script module lang="ts">
	/**
	 * The two sides /for sends people to. `transition` names the element that stands for a side on every /for page (a
	 * door's label, the current crumb, the switch to the other side), so moving between them morphs one into the other.
	 */
	export const sides = {
		promoters: { href: '/for/promoters', label: 'Promoters', title: 'For promoters', tone: 'text-track-aqua', transition: 'side-promoters' },
		djs: { href: '/for/djs', label: 'DJs', title: 'For DJs', tone: 'text-band-low', transition: 'side-djs' }
	} as const;

	export type Side = keyof typeof sides;
</script>

<script lang="ts">
	/**
	 * A side's mark, drawn in its colour: a night's three slots for promoters, and four bars of a level meter for DJs.
	 * It plays once when it appears, and keeps playing while what it sits in is hovered or focused (`group`).
	 */
	let { side }: { side: Side } = $props();
</script>

<svg viewBox="0 0 16 16" class={['side-mark size-4 shrink-0', sides[side].tone]} aria-hidden="true">
	{#if side === 'promoters'}
		{#each [0, 1, 2] as slot (slot)}<rect class="slot" x={1 + slot * 4.5} y={2 + slot * 4.5} width="5.5" height="3" rx="1" fill="currentColor" style:animation-delay="{slot * 120}ms" />{/each}
	{:else}
		{#each [9, 13, 6, 11] as height, bar (bar)}<rect class="bar" x={1.5 + bar * 3.5} y={15 - height} width="2.5" {height} rx="1" fill="currentColor" style:animation-delay="{bar * 90}ms" />{/each}
	{/if}
</svg>

<style>
	/* One keyframe each: it plays once as the page opens, and keeps going while what the mark sits in is hovered. The
	   curve is CSS's ease in and out written as numbers: Tailwind reads the keyword in this file as its class of the same
	   name and adds a rule for it to every page's CSS. */
	.bar {
		transform-box: fill-box;
		transform-origin: bottom;
		animation: level 260ms cubic-bezier(0.42, 0, 0.58, 1) 2 alternate;
	}
	.slot {
		animation: slot 600ms cubic-bezier(0.42, 0, 0.58, 1);
	}
	:global(.group:is(:hover, :focus-within)) :is(.bar, .slot) {
		animation-iteration-count: infinite;
	}
	@keyframes level {
		to {
			transform: scaleY(0.35);
		}
	}
	@keyframes slot {
		50% {
			opacity: 0.35;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.bar,
		.slot {
			animation: none;
		}
	}
</style>
