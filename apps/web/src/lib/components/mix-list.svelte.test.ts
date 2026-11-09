import { page } from 'vitest/browser';
import type { z } from 'zod';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { djSetSchema } from '#lib/dj-schema.ts';
import type { DjSetView } from '#lib/types.ts';
import { silence, sources } from './mix/player-fixtures.ts';
import DjSets from './DjSets.svelte';
import LinkedSet from './LinkedSet.svelte';
import MixList from './MixList.svelte';

const SET_URI = 'at://did:plc:neon/computer.localnetwork.dj.set/3kset';

/** The origins of frames the page was refused. */
const refused: string[] = [];

// Platform players never load here: this page's policy refuses every frame, so opening one fetches nothing.
beforeAll(() => {
	const policy = document.createElement('meta');
	policy.httpEquiv = 'Content-Security-Policy';
	policy.content = "frame-src 'none'";
	document.head.append(policy);
	document.addEventListener('securitypolicyviolation', (event) => refused.push(event.blockedURI));
});

afterEach(() => {
	vi.unstubAllGlobals();
});

describe('MixList', () => {
	it('opens the platform player under a mix, and closes it again', async () => {
		await render(MixList, { mixes: [{ title: 'Tidal Hours', url: 'https://soundcloud.com/neon/tidal-hours' }] });

		await page.getByRole('button', { name: 'Play Tidal Hours' }).click();
		await expect.element(page.getByTitle('Tidal Hours')).toBeInTheDocument();
		await vi.waitFor(() => expect(refused).toContain('https://w.soundcloud.com'));

		// A close mark while it's open, not a pause: whether the platform's player plays is up to it.
		const close = page.getByRole('button', { name: 'Close the player for Tidal Hours' });
		expect(close.element().querySelector('.lucide-x')).not.toBeNull();
		expect(close.element().querySelector('.lucide-pause')).toBeNull();
		await close.click();
		await expect.element(page.getByTitle('Tidal Hours')).not.toBeInTheDocument();
		await expect
			.element(page.getByRole('button', { name: 'Play Tidal Hours' }))
			.toHaveAttribute('aria-expanded', 'false');
	});

	it('gives lengths in minutes or hours, and the year', async () => {
		await render(MixList, {
			mixes: [
				{ title: 'Warm-up', url: 'https://neon.example/warm-up', durationMinutes: 45 },
				{ title: 'All night', url: 'https://www.mixcloud.com/neon/all-night/', durationMinutes: 120, year: 2025 }
			]
		});

		await expect.element(page.getByText('neon.example · 45 min')).toBeInTheDocument();
		await expect.element(page.getByText('Mixcloud · 2 h · 2025')).toBeInTheDocument();
		await expect.element(page.getByRole('button', { name: 'Play Warm-up' })).not.toBeInTheDocument();
	});

	it('leaves out the details line when a link names no site', async () => {
		await render(MixList, { mixes: [{ title: 'Tape', url: 'tape' }] });

		await expect.element(page.getByText('Tape')).toBeInTheDocument();
		expect(document.querySelector('ul p')).toBeNull();
	});
});

describe('LinkedSet', () => {
	const linked = { uri: SET_URI, cid: 'bafylinked' };

	const closing: DjSetView = { uri: SET_URI, cid: 'bafylinked', date: '2026-09-01', trackCount: 1 };

	const show = (set: Partial<DjSetView>) =>
		render(LinkedSet, { linked, sets: Promise.resolve(new Map([[SET_URI, { ...closing, ...set }]])) });

	it('names a set without a title by its venue', async () => {
		await show({ venue: 'Tresor', location: { country: 'DE', locality: 'Berlin' } });

		await expect.element(page.getByRole('link', { name: 'Tresor' })).toBeInTheDocument();
		await expect.element(page.getByText(/Sep 1, 2026 · Berlin · 1 track/)).toBeInTheDocument();
	});

	it('calls a set with neither title nor venue untitled', async () => {
		await show({});

		await expect.element(page.getByRole('link', { name: 'Untitled set' })).toBeInTheDocument();
	});
});

describe('DjSets', () => {
	const set = (value: z.input<typeof djSetSchema>) => ({ ...djSetSchema.parse(value), uri: SET_URI });

	const nowPlaying = () => page.getByRole('region', { name: /^Player for/ });

	it("says when the server can't say where a recording plays", async () => {
		vi.stubGlobal('fetch', async () => new Response('', { status: 502 }));

		await render(DjSets, {
			sets: [set({ date: '2026-09-26', venue: 'Halle Nord', links: [{ uri: 'https://youtu.be/tidal' }], tracks: [] })]
		});
		await page.getByRole('button', { name: 'Play Halle Nord' }).click();

		await expect.element(page.getByText("This recording won't play right now.")).toBeInTheDocument();
	});

	it('names each track the player reaches by its credited artists, or leaves the artist out', async () => {
		vi.stubGlobal('fetch', async () => Response.json({ source: sources(silence(300)).pds }));

		await render(DjSets, {
			sets: [
				set({
					date: '2026-09-26',
					venue: 'Halle Nord',
					links: [{ uri: 'https://archive.org/download/tidal/tidal.mp3' }],
					tracks: [
						{
							title: 'Low Tide (Dub)',
							artists: [{ artistName: 'Ferrule' }, { artistName: 'Ava Lind' }],
							startSeconds: 0
						},
						{ unidentified: true, artist: 'Someone', startSeconds: 60 },
						{ startSeconds: 120 }
					]
				})
			]
		});
		await page.getByRole('button', { name: 'Play Halle Nord' }).click();

		await expect.element(nowPlaying().getByText('Ferrule, Ava Lind', { exact: true })).toBeInTheDocument();
		await page.getByRole('button', { name: 'Play from 1:00: Unreleased' }).click();
		await expect.element(nowPlaying().getByText('Unreleased', { exact: true })).toBeInTheDocument();
		await expect.element(nowPlaying().getByText('Ferrule, Ava Lind')).not.toBeInTheDocument();
		await expect.element(nowPlaying().getByText('Someone')).not.toBeInTheDocument();
		await page.getByRole('button', { name: 'Play from 2:00: Untitled' }).click();
		await expect.element(nowPlaying().getByText('Untitled', { exact: true })).toBeInTheDocument();
	});

	it('names who a set was played with', async () => {
		await render(DjSets, {
			sets: [
				set({
					date: '2026-09-26',
					venue: 'Halle Nord',
					playedWith: [{ name: 'Ava Lind' }, { name: 'Mara Velez' }],
					tracks: []
				})
			]
		});

		await expect.element(page.getByText('Halle Nord · with Ava Lind, Mara Velez · 0 tracks')).toBeInTheDocument();
	});

	it('links a track with no title to MusicBrainz all the same', async () => {
		await render(DjSets, {
			sets: [set({ date: '2026-09-26', tracks: [{ recordingMbId: '0e7a4c19-6d3f-4b82-a1c5-93f2b8e60d17' }] })]
		});

		await page.getByText('Tracklist').click();

		await expect
			.element(page.getByRole('link', { name: 'Track on MusicBrainz' }))
			.toHaveAttribute('href', 'https://musicbrainz.org/recording/0e7a4c19-6d3f-4b82-a1c5-93f2b8e60d17');
	});
});
