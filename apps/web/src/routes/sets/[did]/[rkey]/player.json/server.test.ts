import { afterEach, beforeEach, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import { silentLogger } from '#lib/server/logger.ts';
import { PlyrClient } from '#lib/server/plyr.ts';
import { RecordingService } from '#lib/server/services/recordings.ts';
import { createTestServices } from '#lib/server/testing/core.ts';
import { callRoute } from '#lib/testing/route-event.ts';
import { GET } from './+server.ts';

const DJ = 'did:plc:dj';

let t: Awaited<ReturnType<typeof createTestServices>>;

beforeEach(async () => {
	t = await createTestServices();
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

/** The app, with recordings read through the directory and a plyr.fm that has no tracks. */
const app = () => ({
	...t,
	recordings: new RecordingService(
		t.indexedRepo,
		new PlyrClient(silentLogger, async () => new Response('{}', { status: 404 })),
		async () => true,
		silentLogger
	)
});

/** What the player gets for a set of DJ's linking `links`. */
async function played(rkey: string, links: string[]) {
	await t.net.client(DJ).putRecord({
		collection: COLLECTION.djSet,
		rkey,
		record: {
			$type: COLLECTION.djSet,
			date: '2026-09-12',
			tracks: [{ title: 'Night Bus', artist: 'Ferrule' }],
			links: links.map((uri) => ({ uri })),
			createdAt: '2026-09-13T00:00:00Z'
		}
	});

	return callRoute(GET, { app: app(), params: { did: DJ, rkey } });
}

it("plays a set's archive.org recording as audio, cached for a minute", async () => {
	const archive = 'https://archive.org/download/neon-tidal-hours/tidal-hours.mp3';
	const response = await played('3karchive', ['https://soundcloud.com/neon/tidal-hours', archive]);

	expect(response.headers.get('cache-control')).toBe('public, max-age=60');
	expect(await response.json()).toEqual({
		source: { kind: 'audio', src: archive, from: 'archive.org', link: 'https://archive.org/details/neon-tidal-hours' }
	});
});

it("embeds a platform's player when that's all a set links", async () => {
	const response = await played('3ksoundcloud', ['https://soundcloud.com/neon/tidal-hours']);

	expect(await response.json()).toMatchObject({ source: { kind: 'embed', platform: 'SoundCloud' } });
});

it('has no source for a set with nothing to play, and is a 404 for a set the directory lacks', async () => {
	expect(await (await played('3kplain', [])).json()).toEqual({ source: null });
	await expect(callRoute(GET, { app: app(), params: { did: DJ, rkey: '3kmissing' } })).rejects.toMatchObject({
		status: 404
	});
});
