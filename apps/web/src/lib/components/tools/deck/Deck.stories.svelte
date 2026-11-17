<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, userEvent, within } from 'storybook/test';
	import { phone } from '#lib/testing/viewports.ts';
	import { cuePoints } from '#lib/tools/cues.ts';
	import { demoTrack, tools } from '#lib/tools/index.ts';
	import Deck from './Deck.svelte';

	/** Every tool's hot cues on the demo track, as /tools shows them. */
	const { Story } = defineMeta({ title: 'Patterns/Tool deck', component: Deck, tags: ['autodocs'] });

	// As the page's load places them: every tool's cues along the demo track.
	const track = { ...demoTrack, cues: cuePoints(tools.flatMap((tool) => tool.cues).length, demoTrack.durationSecs, demoTrack.phrases) };

	/** The arrow keys move to the next cue along the track. */
	const arrowsMove = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		const [first, second] = within(canvasElement).getAllByRole('tab');

		await userEvent.click(first);
		await userEvent.keyboard('{ArrowRight}');
		await expect(second).toHaveAttribute('aria-selected', 'true');
		await expect(second).toHaveFocus();
	};
</script>

<Story name="Deck" args={{ tools, track }} play={arrowsMove} />

<Story name="Phone" args={{ tools, track }} globals={phone} />
