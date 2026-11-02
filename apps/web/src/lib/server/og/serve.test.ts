import { describe, expect, it } from 'vitest';
import { silentLogger } from '../logger';
import type { Png } from './render';
import { createCardServer } from './serve';

/** A card of `bytes` bytes. */
const png = (bytes: number): Png => new Uint8Array(bytes);

/** A drawing that finishes when the test says. */
function held() {
	const { promise: done, resolve: finish } = Promise.withResolvers<void>();

	return {
		draw: async () => {
			await done;

			return png(10);
		},
		finish
	};
}

describe('the card server', () => {
	it('keeps the most recently used cards within its byte budget', async () => {
		const server = createCardServer({ budget: 1000, concurrency: 2, waiting: 4 });

		await server.respond(silentLogger, '/a', async () => png(400));
		await server.respond(silentLogger, '/b', async () => png(400));
		// Asking for /a again makes /b the least recently used, so /b goes to make room for /c.
		await server.respond(silentLogger, '/a', async () => png(400));
		await server.respond(silentLogger, '/c', async () => png(400));

		expect([...server.cache.keys()].sort()).toEqual(['/a', '/c']);
		expect(server.cache.calculatedSize).toBeLessThanOrEqual(1000);
		// A card over the whole budget is served but not kept.
		expect((await server.respond(silentLogger, '/huge', async () => png(2000))).status).toBe(200);
		expect(server.cache.has('/huge')).toBe(false);
		expect(server.cache.calculatedSize).toBeLessThanOrEqual(1000);
	});

	it('draws a card once for requests that ask for it at the same time', async () => {
		const server = createCardServer({ budget: 1000, concurrency: 2, waiting: 4 });
		let drawings = 0;

		const draw = async () => {
			drawings += 1;

			return png(10);
		};

		await Promise.all([server.respond(silentLogger, '/a', draw), server.respond(silentLogger, '/a', draw)]);
		expect(drawings).toBe(1);
	});

	it('turns a card away with a 503 once the queue is full, and still serves kept ones', async () => {
		const server = createCardServer({ budget: 1000, concurrency: 1, waiting: 1 });

		await server.respond(silentLogger, '/kept', async () => png(10));
		const first = held();
		const drawing = server.respond(silentLogger, '/drawing', first.draw);
		const waiting = server.respond(silentLogger, '/waiting', async () => png(10));

		const turnedAway = await server.respond(silentLogger, '/turned-away', async () => png(10));

		expect(turnedAway.status).toBe(503);
		expect(turnedAway.headers.get('retry-after')).toBe('5');
		expect(server.cache.has('/turned-away')).toBe(false);
		expect((await server.respond(silentLogger, '/kept', async () => png(10))).status).toBe(200);
		expect(server.kept('/kept')?.headers.get('cache-control')).toBe('public, max-age=31536000, immutable');
		expect(server.kept('/drawing')).toBeNull();

		first.finish();
		expect((await drawing).status).toBe(200);
		expect((await waiting).status).toBe(200);
		// With room again, the turned-away card is drawn.
		expect((await server.respond(silentLogger, '/turned-away', async () => png(10))).status).toBe(200);
	});
});
