<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, waitFor, within } from 'storybook/test';
	import { capture } from '#lib/testing/tools.ts';
	import { phone } from '#lib/testing/viewports.ts';
	import { tools } from '#lib/tools/index.ts';
	import Screen from './Screen.svelte';

	/** The deck's screen: the picked cue's capture, fitted to one fixed shape. */
	const { Story } = defineMeta({ title: 'Patterns/Tool deck screen', component: Screen, tags: ['autodocs'] });

	const captures = tools[0].cues.map((cue) => cue.capture);

	// The second capture never arrives: 192.0.2.1 is reserved for documentation, so nothing answers.
	const stalled = [captures[0], capture('A capture that never arrives', { src: 'http://192.0.2.1/capture.png' }), ...captures.slice(2)];

	/** Once the picked capture settles it's the one showing. */
	const showsPicked = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		const canvas = within(canvasElement);

		await waitFor(() => expect(canvas.getByRole('img', { name: captures[2].alt })).toBeVisible());
	};

	/** Until the picked capture settles, the one before stays up. */
	const keepsFirst = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		await expect(within(canvasElement).getByRole('img', { name: captures[0].alt })).toBeVisible();
	};
</script>

<Story name="First capture" args={{ captures, selected: 0 }} />

<Story name="A later capture" args={{ captures, selected: 2 }} play={showsPicked} />

<Story name="Next capture still loading" args={{ captures: stalled, selected: 1 }} play={keepsFirst} />

<Story name="Phone" args={{ captures, selected: 0 }} globals={phone} />
