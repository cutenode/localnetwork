import { page, userEvent } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { DjSet } from '#lib/dj.ts';
import type { Charts } from '#lib/types.ts';
import DjSets from '../DjSets.svelte';
import ArtistPage from './ArtistPage.svelte';
import ArtistResults from './ArtistResults.svelte';
import ArtistSupport from './ArtistSupport.svelte';
import ChartBoard from './ChartBoard.svelte';
import { artistSupports, charts, supportedArtists } from './fixtures.ts';

/** A promise that never settles, for the moment before data arrives. */
const pending = <T>() => new Promise<T>(() => {});

describe('ChartBoard', () => {
	it('ranks tracks and artists, linking each artist to their page and each recording to MusicBrainz', async () => {
		await render(ChartBoard, { charts, retryHref: '/charts' });

		const tracks = page.getByRole('region', { name: 'Tracks' });
		await expect.element(tracks.getByRole('listitem').first()).toHaveTextContent('1 Night Bus Ferrule 4 DJs');
		await expect
			.element(tracks.getByRole('link', { name: 'Night Bus on MusicBrainz' }))
			.toHaveAttribute('href', 'https://musicbrainz.org/recording/6f1c2d84-3b9e-4c1a-9d57-0e2b8a4f7c31');
		await expect
			.element(tracks.getByRole('link', { name: 'Two Rooms' }))
			.toHaveAttribute('href', '/artists/name/Two%20Rooms');

		const artists = page.getByRole('region', { name: 'Artists' });
		await expect
			.element(artists.getByRole('link', { name: 'Ferrule' }))
			.toHaveAttribute('href', '/artists/2a8e4f60-1c3b-4d7e-9a05-6b1f0c2d3e41');
		await expect.element(artists.getByRole('listitem').first()).toHaveTextContent('1 Ferrule from 4 DJs 9 supports');
	});

	it('shows placeholders while loading, and says when nothing charts or the directory is offline', async () => {
		const screen = await render(ChartBoard, { charts: pending<Charts | null>(), retryHref: '/charts?month=2026-09' });
		await expect.element(page.getByText('Loading tracks')).toBeInTheDocument();

		await screen.rerender({ charts: { ...charts, tracks: [], artists: [] } });
		await expect.element(page.getByText('Nothing charted yet')).toBeVisible();

		await screen.rerender({ charts: null });
		await expect.element(page.getByText('Charts offline')).toBeVisible();
		await expect
			.element(page.getByRole('link', { name: 'Try again' }))
			.toHaveAttribute('href', '/charts?month=2026-09');
	});
});

describe('ArtistSupport', () => {
	it("shows the artist's supports and each play, with the DJ's projects, where, and when in the set", async () => {
		await render(ArtistSupport, { supports: artistSupports, retryHref: '/artists/ferrule' });

		await expect.element(page.getByRole('heading', { level: 1, name: 'Ferrule' })).toBeVisible();
		await expect.element(page.getByText('5 all-time supports from 3 DJs')).toBeVisible();
		await expect
			.element(page.getByRole('link', { name: 'MusicBrainz' }))
			.toHaveAttribute('href', 'https://musicbrainz.org/artist/2a8e4f60-1c3b-4d7e-9a05-6b1f0c2d3e41');

		const plays = page.getByRole('list', { name: 'Plays of Night Bus' }).getByRole('listitem');
		await expect
			.element(plays.first())
			.toHaveTextContent('Tue 13 Oct Neon Tide, Saffron Social @neon.test Halle Nord, Berlin club 0:00 in the set');
		await expect
			.element(plays.first().getByRole('link', { name: 'Saffron Social' }))
			.toHaveAttribute('href', '/projects/saffron-social');
		// A DJ with no active project and no known handle still shows up, as their DID.
		await expect.element(plays.nth(1)).toHaveTextContent('Sat 10 Oct did:plc:harbour Sunday session club');
		await expect.element(plays).toHaveLength(8);

		const more = page.getByRole('list', { name: 'More plays of Night Bus' });
		// Folded away, so not in the accessibility tree until opened.
		await expect.element(more).not.toBeInTheDocument();
		await userEvent.click(page.getByText('2 more plays'));
		await expect.element(more.getByRole('listitem')).toHaveLength(2);
		await expect.element(more).toBeVisible();
	});

	it('says when no set credits the artist or the directory is offline', async () => {
		const screen = await render(ArtistSupport, { supports: { tracks: [], djs: {} }, retryHref: '/artists/ferrule' });

		await expect.element(page.getByText('No plays yet')).toBeVisible();
		await expect.element(page.getByRole('link', { name: 'Find another artist' })).toHaveAttribute('href', '/artists');

		await screen.rerender({ supports: null });
		await expect.element(page.getByText('Artists offline')).toBeVisible();
	});
});

describe('ArtistPage', () => {
	// Its title and canonical address come from the load (see src/lib/server/meta.test.ts).
	it('shows the artist under a way back to all artists', async () => {
		await render(ArtistPage, { supports: artistSupports });

		await expect.element(page.getByRole('link', { name: 'Artists' })).toHaveAttribute('href', '/artists');
		await expect.element(page.getByRole('heading', { level: 1, name: 'Ferrule' })).toBeVisible();
	});
});

describe('ArtistResults', () => {
	it('lists artists with their supports, and says why a search found none', async () => {
		const screen = await render(ArtistResults, { artists: supportedArtists, q: '', retryHref: '/artists' });

		await expect.element(page.getByRole('heading', { name: 'Most supported' })).toBeVisible();
		await expect.element(page.getByRole('listitem').first()).toHaveTextContent('Ferrule 9 all-time supports · 4 DJs');
		await expect
			.element(page.getByRole('link', { name: 'Ferrule on MusicBrainz' }))
			.toHaveAttribute('href', 'https://musicbrainz.org/artist/2a8e4f60-1c3b-4d7e-9a05-6b1f0c2d3e41');

		await screen.rerender({ artists: [], q: 'zzz' });
		await expect.element(page.getByRole('heading', { name: 'Artists matching “zzz”' })).toBeVisible();
		await expect.element(page.getByText('No artists match')).toBeVisible();

		await screen.rerender({ artists: [], q: '' });
		await expect.element(page.getByText('No plays yet')).toBeVisible();

		await screen.rerender({ artists: null, q: 'ferr' });
		await expect.element(page.getByText('Artists offline')).toBeVisible();
	});
});

describe('DjSets', () => {
	it("links a set's tracks to MusicBrainz and its artists to their pages", async () => {
		const set: DjSet = {
			uri: 'at://did:plc:neon/computer.localnetwork.dj.set/3kset',
			date: '2026-10-13',
			tracks: [
				{
					title: 'Low Tide (Dub)',
					artist: 'Ferrule, Ava Lind',
					artists: [
						{ artistName: 'Ferrule', artistMbId: '2a8e4f60-1c3b-4d7e-9a05-6b1f0c2d3e41' },
						{ artistName: 'Ava Lind' }
					],
					recordingMbId: '0e7a4c19-6d3f-4b82-a1c5-93f2b8e60d17'
				},
				{ title: 'Static Bloom', artist: 'Kestrel Unit' },
				{ unidentified: true, artist: 'Someone' }
			]
		};

		await render(DjSets, { sets: [set] });
		await userEvent.click(page.getByText('Tracklist'));

		await expect
			.element(page.getByRole('link', { name: 'Low Tide (Dub) on MusicBrainz' }))
			.toHaveAttribute('href', 'https://musicbrainz.org/recording/0e7a4c19-6d3f-4b82-a1c5-93f2b8e60d17');
		await expect
			.element(page.getByRole('link', { name: 'Ferrule' }))
			.toHaveAttribute('href', '/artists/2a8e4f60-1c3b-4d7e-9a05-6b1f0c2d3e41');
		await expect
			.element(page.getByRole('link', { name: 'Ava Lind' }))
			.toHaveAttribute('href', '/artists/name/Ava%20Lind');
		await expect
			.element(page.getByRole('link', { name: 'Kestrel Unit' }))
			.toHaveAttribute('href', '/artists/name/Kestrel%20Unit');
		await expect.element(page.getByText('Unreleased')).toBeVisible();
		await expect.element(page.getByRole('link', { name: 'Someone' })).not.toBeInTheDocument();
	});
});
