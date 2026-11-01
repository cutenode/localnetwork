import { describe, expect, it } from 'vitest';
import { silentLogger } from './logger';
import { PlyrClient } from './plyr';

const URI = 'at://did:plc:dj/fm.plyr.track/3mylmqbui5222';

/** What plyr.fm's /tracks/by-uri and /tracks/<id> answer, cut down to what the client reads plus a few it ignores. */
type Extra = Partial<{
	gated: boolean;
	support_gate: { type: string };
	visibility: string;
	image_url: string | null;
	atproto_record_uri: string | null;
	atproto_record_cid: string | null;
}>;

const track = (extra: Extra = {}) => ({
	id: 1567,
	title: 'Tidal Hours',
	artist: 'neon.test',
	artist_did: 'did:plc:dj',
	file_id: 'd37d0f0af4c6b364',
	file_type: 'mp3',
	image_url: 'https://images.plyr.fm/images/abc.jpg',
	support_gate: null,
	gated: false,
	visibility: 'public',
	play_count: 5,
	...extra
});

/** A plyr.fm that answers each path from `routes`, 404s anything else, and records what it was asked. */
function plyr(routes: Record<string, () => Response>) {
	const asked: string[] = [];

	const fetch: typeof globalThis.fetch = async (input) => {
		const url = new URL(input instanceof Request ? input.url : input);
		const path = url.pathname + url.search;
		asked.push(path);

		return routes[path]?.() ?? new Response('{"detail":"not found"}', { status: 404 });
	};

	return { asked, fetch };
}

const byUri = `/tracks/by-uri?uri=${encodeURIComponent(URI)}`;

const audioUrl = '/audio/d37d0f0af4c6b364/url';

describe('PlyrClient', () => {
	it("finds a public track and its file on plyr's CDN", async () => {
		const { fetch } = plyr({
			[byUri]: () => Response.json(track()),
			[audioUrl]: () => Response.json({ url: 'https://audio.plyr.fm/audio/d37d0f0af4c6b364.mp3', file_id: 'x' })
		});

		expect(await new PlyrClient(silentLogger, fetch).track(URI)).toEqual({
			id: 1567,
			title: 'Tidal Hours',
			artist: 'neon.test',
			artwork: 'https://images.plyr.fm/images/abc.jpg',
			audio: 'https://audio.plyr.fm/audio/d37d0f0af4c6b364.mp3'
		});
	});

	it("has no audio for a gated or private track, and doesn't ask for it", async () => {
		for (const extra of [{ gated: true }, { support_gate: { type: 'copyright' } }, { visibility: 'private' }]) {
			const { fetch, asked } = plyr({ [byUri]: () => Response.json(track({ ...extra, image_url: null })) });
			const found = await new PlyrClient(silentLogger, fetch).track(URI);

			expect(found).toEqual({
				id: 1567,
				title: 'Tidal Hours',
				artist: 'neon.test',
				artwork: undefined,
				audio: undefined
			});
			expect(asked).toEqual([byUri]);
		}
	});

	it("takes no audio from anywhere but plyr's CDN, or when the lookup fails", async () => {
		for (const answer of [
			() => Response.json({ url: 'https://pds.example/blobs/x.mp3' }),
			() => Response.json({ url: 'https://audio.plyr.fm.example/audio/x.mp3' }),
			() => new Response('down', { status: 503 })
		]) {
			const { fetch } = plyr({ [byUri]: () => Response.json(track()), [audioUrl]: answer });

			expect((await new PlyrClient(silentLogger, fetch).track(URI))?.audio).toBeUndefined();
		}
	});

	it("reads a track plyr.fm doesn't have as null, and reuses answers for a few minutes", async () => {
		let now = 0;
		const { fetch, asked } = plyr({});
		const client = new PlyrClient(silentLogger, fetch, () => now);

		expect(await client.track(URI)).toBeNull();
		expect(await client.track(URI)).toBeNull();
		expect(asked).toHaveLength(1);

		now += 5 * 60_000;
		await client.track(URI);
		expect(asked).toHaveLength(2);
	});

	it("doesn't keep a failure, so the next play asks again", async () => {
		const { fetch, asked } = plyr({ [byUri]: () => new Response('down', { status: 500 }) });
		const client = new PlyrClient(silentLogger, fetch);

		expect(await client.track(URI)).toBeNull();
		expect(await client.track(URI)).toBeNull();
		expect(asked).toEqual([byUri, byUri]);
	});

	it("finds a track's record by its ID, and none it hasn't written or can't name", async () => {
		const CID = 'bafyreihrc5vnkuv5mfz3szvu4bfrtucsxvpu5ms7thsuyfwexmx5gpoyzm';

		const named = (uri: string | null, cid: string | null) => () =>
			Response.json(track({ atproto_record_uri: uri, atproto_record_cid: cid }));

		const { fetch, asked } = plyr({
			'/tracks/1567': named(URI, CID),
			'/tracks/1': named(URI, null),
			'/tracks/2': named(null, CID),
			'/tracks/3': () => new Response('down', { status: 500 })
		});

		const client = new PlyrClient(silentLogger, fetch);

		expect(await client.record('1567')).toEqual({ uri: URI, cid: CID });

		for (const id of ['1', '2', '3', '404']) expect(await client.record(id)).toBeNull();
		expect(asked).toEqual(['/tracks/1567', '/tracks/1', '/tracks/2', '/tracks/3', '/tracks/404']);
	});

	it('forgets the oldest answer past its limit', async () => {
		const { fetch, asked } = plyr({});
		const client = new PlyrClient(silentLogger, fetch);

		for (let i = 0; i <= 200; i++) await client.track(`at://did:plc:dj/fm.plyr.track/${i}`);
		await client.track('at://did:plc:dj/fm.plyr.track/200');
		expect(asked).toHaveLength(201);

		await client.track('at://did:plc:dj/fm.plyr.track/0');
		expect(asked).toHaveLength(202);
	});
});
