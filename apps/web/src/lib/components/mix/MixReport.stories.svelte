<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, userEvent, within } from 'storybook/test';
	import type { MixReport as Report } from '#lib/mix/report.ts';
	import { riddenReport, sampleReport } from '#lib/mix/sample.ts';
	import MixReport from './MixReport.svelte';

	const { Story } = defineMeta({ title: 'Mixes/Mix report', component: MixReport, tags: ['autodocs'], args: { report: sampleReport } });

	const phone = { viewport: { value: 'mobile2', isRotated: false } };

	/** The light palette (packages/tokens/theme.css), which applies under a `light` class on the root. */
	const light = () => {
		document.documentElement.classList.add('light');

		return () => document.documentElement.classList.remove('light');
	};

	/** The sample's five tracks three times over, so the track colours come round again under two-digit numbers. */
	const longReport: Report = {
		...sampleReport,
		plays: Array.from({ length: 15 }, (_, i) => ({ ...sampleReport.plays[i % 5], start: i * 116, end: i * 116 + 126 })),
		transitions: [],
		unidentified: []
	};

	type Context = { canvasElement: HTMLElement };

	/** Picks a track by clicking its row away from the title, then lets it go from the keyboard on the title. */
	const pickRow = async ({ canvasElement }: Context) => {
		const canvas = within(canvasElement);
		const title = canvas.getByRole('button', { name: '2 Signal Path' });

		await userEvent.click(canvas.getAllByText('Floor Two')[0]);
		await expect(title).toHaveAttribute('aria-pressed', 'true');

		title.focus();
		await userEvent.keyboard('{Enter}');
		await expect(title).toHaveAttribute('aria-pressed', 'false');
	};

	/** Each timeline lane is named by what it shows, then the artist, times and confidence; on a phone, where it shows no text, by those alone. */
	const laneNames = async ({ canvasElement }: Context) => {
		const timeline = within(canvasElement.querySelector<HTMLElement>('section[aria-labelledby="mix-timeline"]')!);

		await expect(timeline.getByRole('button', { name: '1 Night Bus by Ferrule, 0:00 to 5:30' })).toBeInTheDocument();
		await expect(timeline.getByRole('button', { name: '5 Static Bloom by Kestrel Unit, 22:40 to 28:40, tentative' })).toBeInTheDocument();
	};

	/** Picks a key on the wheel from the keyboard, which selects its first track. The keys not played stay out of what a screen reader reads. */
	const pickKey = async ({ canvasElement }: Context) => {
		const canvas = within(canvasElement);
		const wheel = within(canvas.getByRole('group', { name: 'Keys as played, in order, on the Camelot wheel' }));

		await expect(wheel.queryByText('1A', { ignore: '[aria-hidden="true"] *' })).toBeNull();

		const track = canvas.getByRole('button', { name: '3 Halogen' });

		wheel.getByRole('button', { name: '9A: track 3' }).focus();
		await userEvent.keyboard('a');
		await expect(track).toHaveAttribute('aria-pressed', 'false');
		await userEvent.keyboard('{Enter}');
		await expect(track).toHaveAttribute('aria-pressed', 'true');
	};
</script>

<Story name="A set" />

<Story name="A set, light" beforeEach={light} />

<Story name="A long set" args={{ report: longReport }} />

<Story name="A long set, light" args={{ report: longReport }} beforeEach={light} />

<Story name="Ridden, on a phone" args={{ report: riddenReport }} globals={phone} />

<Story name="Ridden, on a phone, light" args={{ report: riddenReport }} globals={phone} beforeEach={light} />

<Story name="Lanes named by what they show" play={laneNames} />

<Story name="Lanes named by what they show, on a phone" play={laneNames} globals={phone} />

<Story name="Track picked from its row" play={pickRow} />

<Story name="Key picked from the keyboard" play={pickKey} />
