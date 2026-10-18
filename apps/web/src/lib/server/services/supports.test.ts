import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { HappyViewClient } from '../happyview/client';
import { silentLogger } from '../logger';
import { COLLECTION } from '#lib/nsid.ts';
import { chartFilters } from '#lib/supports.ts';
import { createTestServices } from '../testing/core';
import { SupportService } from './supports';

const ADMIN = 'did:plc:admin';

const DJS = ['did:plc:a', 'did:plc:b', 'did:plc:c'];

const SEPTEMBER = new Date('2026-09-20T12:00:00Z');

const ferrule = { artistName: 'Ferrule', artistMbId: 'mbid:2a8e4f60-1c3b-4d7e-9a05-6b1f0c2d3e41' };

const nightBus = { title: 'Night Bus', artist: 'Ferrule', artists: [ferrule], isrc: 'DEA622600101' };

describe('SupportService', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;
	let rkey = 0;

	/** What locallibrary's `sets publish` writes, straight to the DJ's repo, reaching HappyView over Jetstream. */
	const play = (did: string, date: string, locality: string, tracks: object[]) =>
		t.net.client(did).putRecord({
			collection: COLLECTION.djSet,
			rkey: `3kset${(rkey += 1)}`,
			record: {
				$type: COLLECTION.djSet,
				date,
				kind: 'club',
				venue: 'Halle Nord',
				location: { $type: 'community.lexicon.location.address', name: 'Halle Nord', locality, country: 'DE' },
				tracks,
				createdAt: '2026-10-01T00:00:00Z'
			}
		});

	beforeEach(async () => {
		t = await createTestServices({ admins: [ADMIN], capable: [ADMIN] });
		const actor = await t.viewer(ADMIN);

		for (const [i, name] of ['Neon Tide', 'Saffron Social', 'Pirate Signal'].entries()) {
			await t.projectAdmin.create({
				actor,
				profile: { name, kind: 'solo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } },
				managers: [DJS[i]]
			});
		}
	});
	afterEach(async () => {
		await t.audit.idle();
		await t.db.destroy();
	});

	it('charts a month in one city, and lists the cities', async () => {
		for (const did of DJS) {
			await play(did, '2026-09-12', 'Berlin', [nightBus]);
			await play(did, '2026-08-30', 'Berlin', [{ title: 'August', artist: 'Ferrule' }]);
			await play(did, '2026-09-13', 'Potsdam', [{ title: 'Potsdam', artist: 'Ferrule' }]);
		}

		const charts = await t.supports.charts(
			chartFilters(new URLSearchParams('month=2026-09&place=berlin, de'), SEPTEMBER)
		);

		expect(charts.tracks.map((track) => [track.title, track.supports])).toEqual([['Night Bus', 3]]);
		expect(charts.artists).toEqual([
			{ name: 'Ferrule', mbid: '2a8e4f60-1c3b-4d7e-9a05-6b1f0c2d3e41', supports: 3, djCount: 3 }
		]);
		expect(charts.places).toEqual([
			{ city: 'Berlin', country: 'DE', count: 6 },
			{ city: 'Potsdam', country: 'DE', count: 3 }
		]);
	});

	it("shows an artist's plays with each DJ's handle and projects", async () => {
		await t.accounts.recordLogin({
			did: DJS[0],
			handle: 'neon.test',
			pdsUrl: null,
			grantedScope: 'atproto',
			spacesCapable: false,
			publicWrites: false
		});
		const set = await play(DJS[0], '2026-09-12', 'Berlin', [{ ...nightBus, startSeconds: 300 }]);

		const supports = await t.supports.artist({ name: 'Ferrule' });

		expect(supports.artist).toEqual({
			name: 'Ferrule',
			mbid: '2a8e4f60-1c3b-4d7e-9a05-6b1f0c2d3e41',
			supports: 1,
			djCount: 1
		});
		expect(supports.tracks[0].plays).toEqual([
			{ did: DJS[0], startSeconds: 300, set: expect.objectContaining({ uri: set.uri, date: '2026-09-12' }) }
		]);
		expect(supports.djs).toEqual({
			[DJS[0]]: { did: DJS[0], handle: 'neon.test', projects: [{ slug: 'neon-tide', name: 'Neon Tide' }] }
		});
	});

	it('finds artists by name, and nothing for one no set credits', async () => {
		await play(DJS[0], '2026-09-12', 'Berlin', [nightBus, { title: 'Halogen', artist: 'Two Rooms' }]);

		expect((await t.supports.search('ferr')).map((artist) => artist.name)).toEqual(['Ferrule']);
		expect((await t.supports.search('')).map((artist) => artist.name).sort()).toEqual(['Ferrule', 'Two Rooms']);
		expect(await t.supports.artist({ name: 'Nobody' })).toEqual({ tracks: [], djs: {} });
	});

	it('lists every manager once, whatever their projects say', async () => {
		const actor = await t.viewer(ADMIN);

		await t.projectAdmin.create({
			actor,
			profile: { name: 'Halflight', kind: 'duo', genres: ['house'], location: { city: 'Potsdam', country: 'DE' } },
			managers: [DJS[0], 'did:plc:d']
		});

		expect((await t.projects.managers()).sort()).toEqual([...DJS, 'did:plc:d']);
	});

	it('reuses an answer for a minute, and asks again after a failure', async () => {
		let asked = 0;
		let down = false;
		let now = 0;

		const directory = new HappyViewClient({
			url: 'http://happyview.test',
			fetch: async (input, init) => {
				asked += 1;

				return down ? new Response('down', { status: 503 }) : t.happyView.fetch(new Request(input, init));
			}
		});

		const supports = new SupportService(directory, t.accounts, silentLogger, () => now);
		await supports.search('ferr');
		await supports.search('ferr');
		await supports.search('kest');
		expect(asked).toBe(2);

		now = 60_000;
		down = true;
		await expect(supports.search('ferr')).rejects.toThrow('HTTP 503');
		down = false;
		await supports.search('ferr');
		expect(asked).toBe(4);
	});
});
