<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, fn, waitFor, within } from 'storybook/test';
	import { capture } from '#lib/testing/tools.ts';
	import { phone } from '#lib/testing/viewports.ts';
	import { tools } from '#lib/tools/index.ts';
	import Capture from './Capture.svelte';

	/** A tool at work: the real captures, with the one taken at a phone's width on a phone. */
	const { Story } = defineMeta({ title: 'Patterns/Tool capture', component: Capture, tags: ['autodocs'] });

	const [player] = tools[0].cues;

	// Not an image, so the browser gives up on it and shows the alt text.
	const broken = capture('The drives view', { src: 'data:image/png;base64,AAAA' });

	/** A capture that fails still settles, so whatever waits on it moves on. */
	const settles = async ({ args, canvasElement }: { args: { onsettle?: () => void }; canvasElement: HTMLElement }) => {
		await expect(within(canvasElement).getByRole('img', { name: 'The drives view' })).toBeInTheDocument();
		await waitFor(() => expect(args.onsettle).toHaveBeenCalled());
	};
</script>

<Story name="Capture" args={{ capture: player.capture, eager: true }} />

<Story name="Failed to load" args={{ capture: broken, onsettle: fn() }} play={settles} />

<Story name="Phone" args={{ capture: player.capture, eager: true }} globals={phone} />
