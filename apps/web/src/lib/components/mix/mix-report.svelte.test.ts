import { createRawSnippet } from 'svelte';
import { page, userEvent } from 'vitest/browser';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { MixReport as Report } from '#lib/mix/report.ts';
import { riddenReport, sampleReport } from '#lib/mix/sample.ts';
import KeyWheel from './KeyWheel.svelte';
import MixReport from './MixReport.svelte';
import MixTimeline from './MixTimeline.svelte';
import MixTracklist from './MixTracklist.svelte';
import TempoChart from './TempoChart.svelte';

beforeEach(() => page.viewport(1280, 800));

afterEach(() => page.viewport(414, 896));

describe('MixReport', () => {
	it("offers to play the mix's link where it has a player, and opens it there too", async () => {
		await render(MixReport, { report: sampleReport, listen: 'https://www.mixcloud.com/neon/sunday-warm-up/' });

		await expect.element(page.getByRole('button', { name: 'Play' })).toHaveAttribute('aria-expanded', 'false');
		await expect
			.element(page.getByRole('link', { name: 'Open on Mixcloud' }))
			.toHaveAttribute('href', 'https://www.mixcloud.com/neon/sunday-warm-up/');
	});

	it('leads with the link when nothing plays here, and leaves out a byline it has nothing for', async () => {
		const { title, durationSecs } = sampleReport.mix;

		await render(MixReport, {
			report: { ...sampleReport, mix: { title, durationSecs } },
			listen: 'https://neon.example/sunday'
		});

		await expect.element(page.getByRole('link', { name: 'Listen on neon.example' })).toHaveClass('btn-primary');
		await expect.element(page.getByRole('button', { name: 'Play' })).not.toBeInTheDocument();
		expect(page.getByRole('heading', { level: 1 }).element().nextElementSibling?.querySelectorAll('p')).toHaveLength(1);
	});

	it('puts the project in place of the artist the export names, with its own actions', async () => {
		const project = createRawSnippet(() => ({ render: () => '<a href="/@neon.test">Neon Tide</a>' }));
		const actions = createRawSnippet(() => ({ render: () => '<a href="/edit">Edit</a>' }));

		await render(MixReport, { report: sampleReport, title: 'Tidal Hours', project, actions });

		await expect.element(page.getByRole('link', { name: 'Neon Tide' })).toBeInTheDocument();
		await expect.element(page.getByText('Sample DJ')).not.toBeInTheDocument();
		await expect.element(page.getByRole('heading', { level: 1, name: 'Tidal Hours' })).toBeInTheDocument();
		await expect.element(page.getByRole('link', { name: 'Edit' })).toBeInTheDocument();
		await expect.element(page.getByRole('button', { name: 'Play' })).not.toBeInTheDocument();
	});

	it('shows a date it cannot read as the export has it', async () => {
		await render(MixReport, { report: { ...sampleReport, mix: { ...sampleReport.mix, date: 'late summer' } } });

		await expect.element(page.getByText('late summer · Recorded on XDJ-AZ · House')).toBeInTheDocument();
	});

	it('lays out a sparsely tagged export with what it has', async () => {
		await render(MixReport, { report: sparse });

		await expect.element(page.getByRole('heading', { level: 1, name: 'Untitled recording' })).toBeInTheDocument();
		await expect.element(page.getByText('late summer', { exact: true })).toBeInTheDocument();
		await expect.element(page.getByText('2 stretches not in the library')).toBeInTheDocument();

		const id = page.getByRole('button', { name: '1. ID, 0:00 to 5:30', exact: true });
		await expect.element(id.getByText('Pitched −1.2 st')).toBeInTheDocument();
		await expect.element(id.getByText('2 jumps or loops')).toBeInTheDocument();
		await expect.element(id.getByText('Tentative')).toBeInTheDocument();
		await expect.element(id.getByRole('img', { name: 'Played 6:40 to 1:40 of 1:40' })).toBeInTheDocument();
		await expect.element(id.getByText('6:40 in')).toBeInTheDocument();
		await expect.element(id.getByText('1:40 out / 1:40')).toBeInTheDocument();
		await expect.element(id.getByTitle('Hot cue A at 0:20')).toBeInTheDocument();
		await expect.element(page.getByRole('listitem', { name: 'Transition: quick mix' })).toBeInTheDocument();

		// The timeline names each track as the tracklist does, so the tracklist's row is the last.
		const signal = page.getByRole('button', { name: '2. Signal Path, 5:00 to 10:40', exact: true }).last();
		await expect.element(signal.getByText('from 8A')).toBeInTheDocument();
		expect(signal.element().querySelector('img')?.getAttribute('src')).toBe(PIXEL);

		await page.getByRole('button', { name: '2. Signal Path, 5:00 to 10:40', exact: true }).first().click();
		await expect.element(page.getByText('recorded at 126.0', { exact: false })).toBeInTheDocument();
	});

	it('gives a ridden set its range of BPMs, and a long key path its ends', async () => {
		const ridden: Report = {
			...riddenReport,
			mix: { ...riddenReport.mix, cover: PIXEL },
			plays: riddenReport.plays.map((play, i) => (i === 0 ? { ...play, playedKey: '7A' } : play))
		};

		await render(MixReport, { report: ridden });

		await expect.element(page.getByText('124–134 BPM')).toBeInTheDocument();
		await expect.element(page.getByText('7A → … → 3B')).toBeInTheDocument();
		expect(document.querySelector('header img')?.getAttribute('src')).toBe(PIXEL);
	});

	it('says when nothing from the library was found', async () => {
		await render(MixReport, { report: { ...sampleReport, coverage: 0, plays: [], transitions: [], unidentified: [] } });

		await expect.element(page.getByText('No tracks from the library were found in this mix.')).toBeInTheDocument();
		await expect.element(page.getByText('none found')).toBeInTheDocument();
		await expect.element(page.getByText('nothing missing')).toBeInTheDocument();
		await expect.element(page.getByText('keys unknown')).toBeInTheDocument();
		await expect.element(page.getByText('The library has no BPMs for these tracks.')).toBeInTheDocument();
	});
});

describe('MixTracklist', () => {
	const row = (name: RegExp) => page.getByRole('button', { name });

	it('selects a track and lets it go again, by click or from the keyboard', async () => {
		await render(MixTracklist, { report: sampleReport });

		await row(/^1\. Night Bus/).click();
		await expect.element(row(/^1\. Night Bus/)).toHaveAttribute('aria-pressed', 'true');
		await row(/^1\. Night Bus/).click();
		await expect.element(row(/^1\. Night Bus/)).toHaveAttribute('aria-pressed', 'false');

		await userEvent.keyboard('{Enter}');
		await expect.element(row(/^1\. Night Bus/)).toHaveAttribute('aria-pressed', 'true');
		await userEvent.keyboard(' ');
		await expect.element(row(/^1\. Night Bus/)).toHaveAttribute('aria-pressed', 'false');
		await userEvent.keyboard('a');
		await expect.element(row(/^1\. Night Bus/)).toHaveAttribute('aria-pressed', 'false');
	});

	it('marks the track the player is at', async () => {
		await render(MixTracklist, { report: sampleReport, playing: 2 });

		await expect.element(row(/^Playing now: 3\. Halogen by Two Rooms/)).toBeInTheDocument();
		await expect.element(row(/^1\. Night Bus/)).toBeInTheDocument();
	});
});

describe('KeyWheel', () => {
	const status = () => page.getByText(/keys?\. Point at one|: \d\. /);

	it('names the tracks heard in a key while it is pointed at or focused', async () => {
		await render(KeyWheel, { report: sampleReport });
		const key = page.getByRole('button', { name: '8A: tracks 1, 2' });

		await expect.element(status()).toHaveTextContent('4 keys. Point at one for the tracks heard in it.');
		await userEvent.hover(key);
		await expect.element(status()).toHaveTextContent('8A: 1. Night Bus, 2. Signal Path');
		await userEvent.unhover(key);
		await expect.element(status()).toHaveTextContent('4 keys. Point at one for the tracks heard in it.');

		await key.click();
		await expect.element(key).toHaveAttribute('aria-pressed', 'true');
		await expect.element(status()).toHaveTextContent('8A: 1. Night Bus, 2. Signal Path');
		await userEvent.tab();
		await expect.element(status()).toHaveTextContent('9A: 3. Halogen');
		await userEvent.click(page.getByText('Harmonic move'));
		await expect.element(status()).toHaveTextContent('4 keys. Point at one for the tracks heard in it.');
	});

	it('counts a single key, and forgets a key no longer in the report', async () => {
		const one: Report = { ...sampleReport, plays: sampleReport.plays.slice(0, 1), transitions: [] };
		const screen = await render(KeyWheel, { report: one });
		const key = page.getByRole('button', { name: '8A: track 1' });

		await expect.element(status()).toHaveTextContent('1 key. Point at one for the tracks heard in it.');
		await userEvent.hover(key);
		await expect.element(status()).toHaveTextContent('8A: 1. Night Bus');

		await screen.rerender({ report: { ...one, plays: [{ ...one.plays[0], playedKey: '9A' }] } });
		await expect.element(page.getByText(/^\s*8A:\s*$/)).toBeInTheDocument();
		await expect.element(page.getByText('8A: 1. Night Bus')).not.toBeInTheDocument();
	});

	it('says when the library has no keys', async () => {
		const unkeyed = { ...sampleReport, plays: sampleReport.plays.map((play) => ({ ...play, playedKey: undefined })) };

		await render(KeyWheel, { report: unkeyed });

		await expect.element(page.getByText('The library has no keys for these tracks.').last()).toBeVisible();
	});
});

describe('MixTimeline', () => {
	/** The mix's waveform and lanes, which follow the pointer. */
	// SAFETY: the waveform is always a child of the box that follows the pointer.
	const lanes = () => page.getByRole('img', { name: /^The mix in three bands/ }).element().parentElement!;

	/** Points at `seconds` into the 29-minute sample. */
	const point = (seconds: number) => {
		const box = lanes().getBoundingClientRect();
		lanes().dispatchEvent(
			new PointerEvent('pointermove', { clientX: box.left + (seconds / 1740) * box.width, bubbles: true })
		);
	};

	it('says what plays where it is pointed at, even where nothing was identified', async () => {
		await render(MixTimeline, { report: sampleReport });

		point(320);
		await expect.element(page.getByText('5:20 · 1. Night Bus + 2. Signal Path')).toBeInTheDocument();
		point(1000);
		await expect.element(page.getByText('16:40 · no track identified')).toBeInTheDocument();
		lanes().dispatchEvent(new PointerEvent('pointerleave'));
		await expect
			.element(page.getByText('Point at the mix to see what’s playing.', { exact: false }))
			.toBeInTheDocument();
	});

	it('zooms in, and picks a track and lets it go', async () => {
		await render(MixTimeline, { report: sampleReport });
		const track = page.getByRole('button', { name: /^3\. Halogen/ });

		await page.getByRole('button', { name: '2×' }).click();
		await expect.element(page.getByRole('button', { name: '2×' })).toHaveAttribute('aria-pressed', 'true');
		await expect.element(page.getByRole('button', { name: 'Fit' })).toHaveAttribute('aria-pressed', 'false');
		await page.getByRole('button', { name: 'Fit' }).click();

		await track.click();
		await expect.element(track).toHaveAttribute('aria-pressed', 'true');
		await track.click();
		await expect.element(track).toHaveAttribute('aria-pressed', 'false');
	});
});

describe('TempoChart', () => {
	it('goes back to its hint once the pointer leaves', async () => {
		await render(TempoChart, { report: sampleReport });
		const chart = page.getByRole('img', { name: /^Tempo of each track/ });

		await userEvent.hover(chart);
		await expect.element(page.getByText('126.0 BPM', { exact: true })).toBeInTheDocument();
		await userEvent.unhover(chart);
		await expect.element(page.getByText('Point at the chart for each track’s tempo.')).toBeInTheDocument();
	});
});

// Helpers

const PIXEL =
	'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=';

const [first, second, third] = sampleReport.plays;

/** An export from a library with little tagged: no artists, labels, keys or BPMs for some tracks, and two stretches it couldn't place. */
const sparse: Report = {
	...sampleReport,
	mix: { title: 'Untitled recording', date: 'late summer', durationSecs: 1200 },
	plays: [
		{
			...first,
			track: { title: 'ID', durationSecs: 0, cues: [{ at: 20, label: 'A' }], phrases: [] },
			trackStart: 400,
			trackEnd: 100,
			keyLock: false,
			semitones: -1.2,
			key: undefined,
			playedKey: undefined,
			playedBpm: undefined,
			jumps: 2,
			presence: [],
			confidence: 0.3
		},
		{
			...second,
			track: {
				...second.track,
				artist: undefined,
				label: undefined,
				bpm: undefined,
				key: undefined,
				overview: undefined,
				artwork: PIXEL
			},
			key: '8A',
			playedKey: '9A',
			keyLock: null,
			semitones: 0
		},
		third
	],
	transitions: [
		{ from: 0, to: 1, at: 300, overlap: 2 },
		{ from: 1, to: 2, at: 610, overlap: 30, harmony: 'clash' }
	],
	unidentified: [
		{ start: 980, end: 1050 },
		{ start: 1100, end: 1200 }
	]
};
