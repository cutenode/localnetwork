import { cidForLex } from '@atproto/lex-cbor';
import { jsonToLex } from '@atproto/lex-json';
import { describe, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import type { JsonRecord } from '../atproto/repo-client';
import { HappyViewClient } from '../happyview/client';
import { IndexedRepo } from '../happyview/indexed';
import { silentLogger } from '../logger';
import { PlyrClient } from '../plyr';
import { FakeHappyView } from '../testing/happyview';
import { RecordingService } from './recordings';

const DJ = 'did:plc:dj';

const STRANGER = 'did:plc:stranger';

const AUDIO_CID = 'bafkreigtpuhqv5ggwnsphu54sjzurjztcspl4xp2t7wcaudioaudioaudio';

const PLYR_URI = `at://${DJ}/fm.plyr.track/3plyr`;

const SOUNDCLOUD = 'https://soundcloud.com/neon/tidal-hours';

const ARCHIVE = 'https://archive.org/download/neon-tidal-hours/tidal-hours.mp3';

/** The recording lexicon's `audio` maxSize: 300 MiB. */
const MAX_SIZE = 314_572_800;

/** A `computer.localnetwork.dj.recording` as locallibrary writes it: an hour long, unless `extra` says otherwise. */
const recording = (
	audio: Partial<{ mimeType: string; size: number; cid: string }> = {},
	extra: JsonRecord = { durationSeconds: 3600 }
): JsonRecord => ({
	$type: COLLECTION.djRecording,
	audio: {
		$type: 'blob',
		ref: { $link: audio.cid ?? AUDIO_CID },
		mimeType: audio.mimeType ?? 'audio/mpeg',
		size: audio.size ?? 1234
	},
	createdAt: '2026-10-26T12:00:00.000Z',
	...extra
});

type Seeded = { did?: string; collection: string; rkey: string; record: JsonRecord };

type Options = { plyrTrack?: { gated?: boolean }; manages?: (did: string) => boolean; now?: () => number };

/**
 * A RecordingService over a HappyView that indexed `records` through their record scripts, a plyr.fm answering `plyrTrack` (with CDN audio unless
 * `gated`), and `DJ` as the only account that manages an active project unless `manages` says otherwise. Counts what
 * it asks.
 */
async function service(records: Seeded[], { plyrTrack, manages = (did) => did === DJ, now }: Options = {}) {
	const hv = new FakeHappyView('did:plc:root');

	for (const { did = DJ, ...rest } of records)
		await hv.ingest({ did, cid: (await cidForLex(jsonToLex(rest.record))).toString(), ...rest });
	const asked = { happyView: 0, plyr: 0, managers: 0 };

	const directory = new HappyViewClient({
		url: 'http://happyview.test',
		fetch: async (input, init) => {
			asked.happyView++;

			return hv.fetch(new Request(input, init));
		}
	});

	const plyrFetch: typeof globalThis.fetch = async (input) => {
		asked.plyr++;
		const url = new URL(input instanceof Request ? input.url : input);

		if (!plyrTrack) return new Response('{}', { status: 404 });

		if (url.pathname === '/tracks/by-uri')
			return Response.json({
				id: 1567,
				title: 'Tidal Hours',
				artist: 'neon.test',
				file_id: 'f1',
				image_url: null,
				gated: plyrTrack.gated ?? false
			});

		return Response.json({ url: 'https://audio.plyr.fm/audio/f1.mp3' });
	};

	const recordings = new RecordingService(
		new IndexedRepo(directory, silentLogger),
		new PlyrClient(silentLogger, plyrFetch),
		async (did) => {
			asked.managers++;

			return manages(did);
		},
		silentLogger,
		now
	);

	return { recordings, asked };
}

const set = (record: JsonRecord, rkey = '3set'): Seeded => ({
	collection: COLLECTION.djSet,
	rkey,
	record: { $type: COLLECTION.djSet, date: '2026-09-26', tracks: [], ...record }
});

const ref = (uri: string) => ({ uri, cid: 'bafyx' });

describe('RecordingService.source', () => {
	it("plays a plyr.fm recording from plyr's CDN, ahead of the set's links", async () => {
		const { recordings } = await service([], { plyrTrack: {} });

		expect(await recordings.source({ recording: ref(PLYR_URI), links: [{ uri: ARCHIVE }] })).toEqual({
			kind: 'audio',
			src: 'https://audio.plyr.fm/audio/f1.mp3',
			from: 'plyr.fm',
			artwork: undefined,
			link: 'https://plyr.fm/track/1567'
		});
	});

	it("falls back to plyr's own player for a gated track, but takes audio from a link over it", async () => {
		const { recordings } = await service([], { plyrTrack: { gated: true } });

		expect(await recordings.source({ recording: ref(PLYR_URI), links: [{ uri: SOUNDCLOUD }] })).toEqual({
			kind: 'embed',
			src: 'https://plyr.fm/embed/track/1567',
			platform: 'plyr.fm',
			link: 'https://plyr.fm/track/1567'
		});
		expect(await recordings.source({ recording: ref(PLYR_URI), links: [{ uri: ARCHIVE }] })).toMatchObject({
			from: 'archive.org'
		});
	});

	it("uses the links when plyr.fm doesn't have the track", async () => {
		const { recordings } = await service([]);

		expect(await recordings.source({ recording: ref(PLYR_URI), links: [{ uri: SOUNDCLOUD }] })).toMatchObject({
			kind: 'embed',
			platform: 'SoundCloud'
		});
		expect(await recordings.source({ recording: ref(PLYR_URI) })).toBeNull();
	});

	it("plays a recording on a manager's PDS through the site's audio route", async () => {
		const { recordings } = await service([{ collection: COLLECTION.djRecording, rkey: '3rec', record: recording() }]);

		expect(await recordings.source({ recording: ref(`at://${DJ}/${COLLECTION.djRecording}/3rec`) })).toEqual({
			kind: 'audio',
			src: `/media/audio/${DJ}/${AUDIO_CID}`,
			from: 'pds'
		});
	});

	it('plays a recording without durationSeconds, which the player reads from the audio', async () => {
		const { recordings } = await service([
			{ collection: COLLECTION.djRecording, rkey: '3rec', record: recording({}, {}) }
		]);

		expect(await recordings.source({ recording: ref(`at://${DJ}/${COLLECTION.djRecording}/3rec`) })).toMatchObject({
			src: `/media/audio/${DJ}/${AUDIO_CID}`
		});
		expect(await recordings.audio(DJ, AUDIO_CID)).toEqual({ mimeType: 'audio/mpeg', size: 1234 });
	});

	it('plays a video/mp4 recording, the type a PDS gives an M4A it reads as video', async () => {
		const { recordings } = await service([
			{ collection: COLLECTION.djRecording, rkey: '3rec', record: recording({ mimeType: 'video/mp4' }) }
		]);

		expect(await recordings.source({ recording: ref(`at://${DJ}/${COLLECTION.djRecording}/3rec`) })).toMatchObject({
			src: `/media/audio/${DJ}/${AUDIO_CID}`
		});
		expect(await recordings.audio(DJ, AUDIO_CID)).toEqual({ mimeType: 'video/mp4', size: 1234 });
	});

	it("takes a recording of the lexicon's maxSize, and not a byte more", async () => {
		const { recordings } = await service([
			{ collection: COLLECTION.djRecording, rkey: 'cap', record: recording({ cid: 'bafycap', size: MAX_SIZE }) },
			{ collection: COLLECTION.djRecording, rkey: 'over', record: recording({ cid: 'bafyover', size: MAX_SIZE + 1 }) }
		]);

		expect(await recordings.source({ recording: ref(`at://${DJ}/${COLLECTION.djRecording}/cap`) })).toMatchObject({
			src: `/media/audio/${DJ}/bafycap`
		});
		expect(await recordings.audio(DJ, 'bafycap')).toEqual({ mimeType: 'audio/mpeg', size: MAX_SIZE });
		expect(await recordings.source({ recording: ref(`at://${DJ}/${COLLECTION.djRecording}/over`) })).toBeNull();
		expect(await recordings.audio(DJ, 'bafyover')).toBeNull();
	});

	it("doesn't offer a PDS recording that isn't audio, is too big, is missing, or isn't a manager's", async () => {
		const { recordings } = await service([
			{ collection: COLLECTION.djRecording, rkey: 'video', record: recording({ mimeType: 'video/webm' }) },
			{ collection: COLLECTION.djRecording, rkey: 'huge', record: recording({ size: MAX_SIZE + 1 }) },
			{ did: STRANGER, collection: COLLECTION.djRecording, rkey: '3rec', record: recording() }
		]);

		for (const uri of [
			`at://${DJ}/${COLLECTION.djRecording}/video`,
			`at://${DJ}/${COLLECTION.djRecording}/huge`,
			`at://${DJ}/${COLLECTION.djRecording}/gone`,
			`at://${STRANGER}/${COLLECTION.djRecording}/3rec`,
			`at://${DJ}/app.bsky.feed.post/3rec`
		])
			expect(await recordings.source({ recording: ref(uri) })).toBeNull();
	});
});

describe('RecordingService.set and forMix', () => {
	it('reads a set through HappyView, and nothing for one it lacks', async () => {
		const { recordings } = await service([set({ title: 'Tidal Hours', links: [{ uri: SOUNDCLOUD }] })]);

		expect(await recordings.set(DJ, '3set')).toMatchObject({
			uri: `at://${DJ}/${COLLECTION.djSet}/3set`,
			title: 'Tidal Hours'
		});
		expect(await recordings.set(DJ, 'gone')).toBeNull();
		expect(await recordings.set('not a did', '3set')).toBeNull();
		expect(await recordings.set(DJ, '3set/../x')).toBeNull();
	});

	it("plays a mix's linked set, taking audio from either the set or the mix's own link", async () => {
		const { recordings } = await service(
			[set({ recording: ref(PLYR_URI) }), set({ links: [{ uri: SOUNDCLOUD }] }, '3embed')],
			{ plyrTrack: {} }
		);

		const setUri = (rkey: string) => ({ uri: `at://${DJ}/${COLLECTION.djSet}/${rkey}`, cid: 'bafyx' });

		expect(await recordings.forMix({ url: SOUNDCLOUD, set: setUri('3set') })).toMatchObject({ from: 'plyr.fm' });
		expect(await recordings.forMix({ url: ARCHIVE, set: setUri('3embed') })).toMatchObject({ from: 'archive.org' });
		expect(await recordings.forMix({ url: 'https://www.mixcloud.com/neon/x/', set: setUri('3embed') })).toMatchObject({
			platform: 'SoundCloud'
		});
		expect(await recordings.forMix({ url: SOUNDCLOUD, set: setUri('gone') })).toMatchObject({ platform: 'SoundCloud' });
		expect(await recordings.forMix({ url: 'https://neon.example/mix' })).toBeNull();
	});
});

describe('RecordingService.audio', () => {
	it('finds the declared type and size of the recording whose blob it is', async () => {
		const { recordings } = await service([
			{ collection: COLLECTION.djRecording, rkey: 'a', record: recording({ cid: 'bafyother' }) },
			{ collection: COLLECTION.djRecording, rkey: 'b', record: recording({ mimeType: 'audio/ogg', size: 999 }) }
		]);

		expect(await recordings.audio(DJ, AUDIO_CID)).toEqual({ mimeType: 'audio/ogg', size: 999 });
		expect(await recordings.audio(DJ, 'bafyunreferenced')).toBeNull();
	});

	it("refuses a blob a non-manager's recording, a video or an oversize one references", async () => {
		const { recordings } = await service([
			{ did: STRANGER, collection: COLLECTION.djRecording, rkey: 'a', record: recording() },
			{
				collection: COLLECTION.djRecording,
				rkey: 'v',
				record: recording({ cid: 'bafyvideo', mimeType: 'video/webm' })
			},
			{ collection: COLLECTION.djRecording, rkey: 'h', record: recording({ cid: 'bafyhuge', size: MAX_SIZE + 1 }) }
		]);

		expect(await recordings.audio(STRANGER, AUDIO_CID)).toBeNull();
		expect(await recordings.audio(DJ, 'bafyvideo')).toBeNull();
		expect(await recordings.audio(DJ, 'bafyhuge')).toBeNull();
	});

	it('reuses a lookup for a minute, since every seek asks again', async () => {
		let now = 0;

		const { recordings, asked } = await service(
			[{ collection: COLLECTION.djRecording, rkey: 'a', record: recording() }],
			{ now: () => now }
		);

		await recordings.audio(DJ, AUDIO_CID);
		const lookups = asked.happyView;
		await recordings.audio(DJ, AUDIO_CID);
		expect(asked).toMatchObject({ happyView: lookups, managers: 1 });

		now += 60_000;
		expect(await recordings.audio(DJ, AUDIO_CID)).toEqual({ mimeType: 'audio/mpeg', size: 1234 });
		expect(asked.happyView).toBeGreaterThan(lookups);
		expect(asked.managers).toBe(2);
	});

	it("reads a manager check that fails as no, and doesn't remember it", async () => {
		let fail = true;

		const { recordings } = await service([{ collection: COLLECTION.djRecording, rkey: 'a', record: recording() }], {
			manages: () => {
				if (fail) throw new Error('directory down');

				return true;
			}
		});

		expect(await recordings.source({ recording: ref(`at://${DJ}/${COLLECTION.djRecording}/a`) })).toBeNull();
		fail = false;
		expect(await recordings.source({ recording: ref(`at://${DJ}/${COLLECTION.djRecording}/a`) })).toMatchObject({
			from: 'pds'
		});
	});
});
