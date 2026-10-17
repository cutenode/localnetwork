import { describe, expect, it } from 'vitest';
import { DEMO_DJ_PROFILE, DEMO_DJ_SETS } from '../../scripts/seed-data.ts';
import { countLabel, type DjSet, setPlace, trackBpm } from './dj.ts';
import { djProfileSchema, djSetSchema } from './dj-schema.ts';

describe('djProfileSchema', () => {
	it('reads a full profile as locallibrary publishes it', () => {
		const profile = djProfileSchema.parse(DEMO_DJ_PROFILE);

		expect(profile.tempo).toEqual({ low: 126, high: 134, median: 130 });
		expect(profile.keys?.top?.map((key) => key.key)).toEqual(['8A', '9A', '7A', '10A']);
		expect(profile.signature?.[0]).toMatchObject({ title: 'Night Bus', bpmCentis: 12400, key: '8A' });
	});

	it('drops a malformed section and keeps the rest', () => {
		const profile = djProfileSchema.parse({ ...DEMO_DJ_PROFILE, tempo: { low: 'fast' }, mixing: { harmonic: 140 } });

		expect(profile.tempo).toBeUndefined();
		expect(profile.mixing).toBeUndefined();
		expect(profile.genres?.[0]).toEqual({ name: 'techno', share: 55 });
	});

	it('needs to know when it was made', () => {
		expect(djProfileSchema.safeParse({ headline: 'Techno' }).success).toBe(false);
	});
});

describe('djSetSchema', () => {
	it('reads a set and its address', () => {
		const set = djSetSchema.parse(DEMO_DJ_SETS[0]);

		expect(set.location).toMatchObject({ name: 'Halle Nord', locality: 'Berlin', country: 'DE' });
		expect(set.tracks).toHaveLength(5);
	});

	it('rejects a set without a proper date or tracks', () => {
		expect(djSetSchema.safeParse({ ...DEMO_DJ_SETS[0], date: 'last week' }).success).toBe(false);
		expect(djSetSchema.safeParse({ date: '2026-09-26' }).success).toBe(false);
	});

	it("keeps a track's IDs and artists, with MusicBrainz IDs as bare lowercase UUIDs", () => {
		const set = djSetSchema.parse({
			...DEMO_DJ_SETS[0],
			tracks: [
				{
					...DEMO_DJ_SETS[0].tracks[0],
					releaseMbId: 'not an id',
					artists: [{ artistName: 'Ferrule', artistMbId: '2A8E4F60-1C3B-4D7E-9A05-6B1F0C2D3E41' }]
				}
			]
		});

		expect(set.tracks[0]).toMatchObject({
			title: 'Night Bus',
			isrc: 'DEA622600101',
			recordingMbId: '6f1c2d84-3b9e-4c1a-9d57-0e2b8a4f7c31',
			artists: [{ artistName: 'Ferrule', artistMbId: '2a8e4f60-1c3b-4d7e-9a05-6b1f0c2d3e41' }]
		});
		expect(set.tracks[0].releaseMbId).toBeUndefined();
	});

	it('leaves out a link that isn’t a URL', () => {
		const set = djSetSchema.parse({ ...DEMO_DJ_SETS[2], links: [{ uri: 'not a url' }] });

		expect(set.links).toBeUndefined();
	});
});

describe('labels', () => {
	it('marks rounded counts', () => {
		expect(countLabel({ atLeast: 3000 })).toBe('3,000+');
		expect(countLabel({ count: 48 })).toBe('48');
		expect(countLabel(undefined)).toBeNull();
	});

	it('names where a set was without repeating itself', () => {
		const set = (value: Partial<Omit<DjSet, 'uri'>>) => ({
			...djSetSchema.parse({ date: '2026-09-26', tracks: [], ...value }),
			uri: 'at://x'
		});

		expect(setPlace(set({ venue: 'Halle Nord', location: { name: 'Halle Nord', locality: 'Berlin' } }))).toBe(
			'Halle Nord, Berlin'
		);
		expect(setPlace(set({ location: { locality: 'Berlin' } }))).toBe('Berlin');
		expect(setPlace(set({}))).toBeNull();
	});

	it('shows a whole BPM without decimals', () => {
		expect(trackBpm(12800)).toBe('128');
		expect(trackBpm(12450)).toBe('124.5');
		expect(trackBpm(undefined)).toBeNull();
	});
});
