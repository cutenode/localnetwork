<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, userEvent, within } from 'storybook/test';
	import { sampleReport } from '#lib/mix/sample.ts';
	import { phone } from '#lib/testing/viewports.ts';
	import MixTimeline from './MixTimeline.svelte';

	const { Story } = defineMeta({ title: 'Mixes/Timeline', component: MixTimeline, tags: ['autodocs'], args: { report: sampleReport } });

	const nothingFound = { ...sampleReport, plays: [], transitions: [], unidentified: [{ start: 0, end: sampleReport.mix.durationSecs }] };

	/** Zooms in to 4x from the zoom buttons. */
	const zoomIn = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		const zoom = within(within(canvasElement).getByRole('group', { name: 'Zoom' }));

		await userEvent.click(zoom.getByRole('button', { name: '4×' }));
		await expect(zoom.getByRole('button', { name: '4×' })).toHaveAttribute('aria-pressed', 'true');
		await expect(zoom.getByRole('button', { name: 'Fit' })).toHaveAttribute('aria-pressed', 'false');
	};
</script>

<Story name="A set" />

<Story name="A track picked" args={{ selected: 1 }} />

<Story name="Zoomed in" play={zoomIn} />

<Story name="Nothing identified" args={{ report: nothingFound }} />

<Story name="Phone" args={{ selected: 1 }} globals={phone} />
