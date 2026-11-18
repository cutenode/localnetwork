<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, within } from 'storybook/test';
	import { light } from '#lib/testing/light.ts';
	import SideMark, { type Side, sides } from './SideMark.svelte';

	/** Each side's mark in its colour, as it sits in a link to that side's page. */
	const { Story } = defineMeta({ title: 'Patterns/Side mark', component: SideMark, tags: ['autodocs'] });

	/** The mark draws in its side's colour, hidden from screen readers next to the link's own name. */
	const draws = (side: Side, pieces: number) =>
		async ({ canvasElement }: { canvasElement: HTMLElement }) => {
			const mark = within(canvasElement).getByRole('link', { name: sides[side].title }).querySelector('svg');

			await expect(mark).toHaveAttribute('aria-hidden', 'true');
			await expect(mark).toHaveClass(sides[side].tone);
			await expect(mark?.querySelectorAll('rect')).toHaveLength(pieces);
		};

	/**
	 * With focus in the link the mark sits in, it keeps playing. The check reads the rule, not a frame, so it holds with
	 * reduced motion too, where the mark doesn't move at all.
	 */
	const playsOn = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		const link = within(canvasElement).getByRole('link', { name: sides.djs.title });

		link.focus();

		for (const bar of link.querySelectorAll('rect')) await expect(getComputedStyle(bar).animationIterationCount).toBe('infinite');
	};
</script>

{#snippet linked(side: Side)}
	<a href={sides[side].href} class="group inline-flex items-center gap-2 font-semibold text-fg no-underline"><SideMark {side} />{sides[side].title}</a>
{/snippet}

<!-- A night's three slots. -->
<Story name="Promoters" asChild play={draws('promoters', 3)}>{@render linked('promoters')}</Story>

<!-- Four bars of a level meter. -->
<Story name="DJs" asChild play={draws('djs', 4)}>{@render linked('djs')}</Story>

<Story name="Both, light" asChild beforeEach={light}>
	<div class="flex gap-6">{@render linked('promoters')}{@render linked('djs')}</div>
</Story>

<Story name="In a focused link" asChild play={playsOn}>{@render linked('djs')}</Story>
