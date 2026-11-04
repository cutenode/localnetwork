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

it("serves a set's timed tracks as Podcasting 2.0 chapters, in order", async () => {
	await t.net.client(DJ).putRecord({
		collection: COLLECTION.djSet,
		rkey: '3kset',
		record: {
			$type: COLLECTION.djSet,
			title: 'Closing at Halle Nord',
			date: '2026-09-12',
			tracks: [
				{ title: 'Dawn Chorus', artist: 'Ostgut Ton', startSeconds: 420 },
				{ title: 'Night Bus', artist: 'Ferrule', startSeconds: 0 },
				{ title: 'Untimed', artist: 'Kiln' }
			],
			createdAt: '2026-09-13T00:00:00Z'
		}
	});

	const response = await callRoute(GET, { app: app(), params: { did: DJ, rkey: '3kset' } });

	expect(response.headers.get('content-type')).toBe('application/json+chapters');
	expect(response.headers.get('cache-control')).toBe('public, max-age=300');
	expect(await response.json()).toEqual({
		version: '1.2.0',
		title: 'Closing at Halle Nord',
		chapters: [
			{ startTime: 0, title: 'Ferrule - Night Bus' },
			{ startTime: 420, title: 'Ostgut Ton - Dawn Chorus' }
		]
	});
});

it("is a 404 for a set the directory doesn't have", async () => {
	await expect(callRoute(GET, { app: app(), params: { did: DJ, rkey: '3kmissing' } })).rejects.toMatchObject({
		status: 404
	});
});
