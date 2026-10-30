import { describe, expect, it } from 'vitest';
import type { z } from 'zod';
import { djSetSchema } from '#lib/dj-schema.ts';
import { setChapters } from './chapters';

const MBID = '0f7c3f2e-6f5a-4b7e-9c4e-1a2b3c4d5e6f';

const set = (value: Partial<z.input<typeof djSetSchema>>) => ({
	...djSetSchema.parse({ date: '2026-09-26', ...value }),
	uri: 'at://did:plc:dj/computer.localnetwork.dj.set/3set'
});

describe('setChapters', () => {
	it('makes a chapter of each track with a start, in order, linking MusicBrainz recordings', () => {
		const chapters = setChapters(
			set({
				title: 'Tidal Hours',
				tracks: [
					{ title: 'Night Bus', artist: 'Halo', startSeconds: 300, recordingMbId: MBID },
					{ title: 'Opener', artists: [{ artistName: 'Ana' }, { artistName: 'Bo' }], startSeconds: 0 },
					{ title: 'No start', artist: 'Halo' },
					{ unidentified: true, startSeconds: 600 },
					{ startSeconds: 900 }
				]
			})
		);

		expect(chapters).toEqual({
			version: '1.2.0',
			title: 'Tidal Hours',
			chapters: [
				{ startTime: 0, title: 'Ana, Bo - Opener' },
				{ startTime: 300, title: 'Halo - Night Bus', url: `https://musicbrainz.org/recording/${MBID}` },
				{ startTime: 600, title: 'Unreleased' },
				{ startTime: 900, title: 'Untitled' }
			]
		});
	});

	it('names a set by where it was, or not at all, and has no chapters without starts', () => {
		expect(setChapters(set({ venue: 'Halle Nord', tracks: [{ title: 'x' }] }))).toEqual({
			version: '1.2.0',
			title: 'Halle Nord',
			chapters: []
		});
		expect(setChapters(set({ tracks: [] }))).toEqual({ version: '1.2.0', chapters: [] });
	});
});
