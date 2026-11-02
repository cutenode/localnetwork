import { LRUCache } from 'lru-cache';
import PQueue from 'p-queue';
import type { Logger } from '../logger';
import type { Png } from './render';

/** Card URLs that name a record's version: the card at one never changes. */
export const IMMUTABLE = 'public, max-age=31536000, immutable';

/**
 * Rendered cards kept in memory, by URL. One replica and no CDN in front means every crawler's fetch reaches the app,
 * and a link shared in a busy chat brings several within a minute. 32 MB holds a few hundred cards (they weigh 20 to
 * 290 KB, up to 600 KB over a noisy photo), enough for the links being shared at any one time, and is small beside the Node heap.
 */
const CACHE_BYTES = 32 * 1024 * 1024;

/**
 * Cards drawn at once. Drawing is CPU on the one event loop, so more at a time finishes none sooner and only holds more
 * decoded photos in memory.
 */
const RENDERS = 2;

/** Cards that may wait their turn. The slowest takes 0.3 s (one over a press photo), so this is a few seconds' wait. */
const WAITING = 16;

/** How long a turned-away crawler is asked to wait, in seconds. */
const RETRY_AFTER = 5;

class Busy extends Error {}

const png = (body: Png, cacheControl: string) =>
	new Response(body, { headers: { 'content-type': 'image/png', 'cache-control': cacheControl } });

/**
 * Serves cards from a cache of `budget` bytes, least recently used out first, drawing a missing one with at most
 * `concurrency` drawing at once and `waiting` more queued. Past that, a request is answered 503 with Retry-After rather
 * than queued without end. A request for a card being drawn waits for that drawing.
 */
export function createCardServer({
	budget,
	concurrency,
	waiting
}: {
	budget: number;
	concurrency: number;
	waiting: number;
}) {
	const cache = new LRUCache<string, Png, () => Promise<Png>>({
		maxSize: budget,
		sizeCalculation: (card) => card.byteLength,
		fetchMethod: (_key, _stale, { context }) => context()
	});

	const queue = new PQueue({ concurrency });

	return {
		cache,

		/** The card at `path` if one is kept, without drawing it. */
		kept(path: string, cacheControl = IMMUTABLE): Response | null {
			const card = cache.get(path);

			return card ? png(card, cacheControl) : null;
		},

		/**
		 * The card at `path`, kept or drawn by `draw` and kept, for `ttl` milliseconds when it can change (an artist's
		 * counts). Each drawing's time goes to the request's log.
		 */
		async respond(
			log: Logger,
			path: string,
			draw: () => Promise<Png>,
			{ cacheControl = IMMUTABLE, ttl }: { cacheControl?: string; ttl?: number } = {}
		): Promise<Response> {
			const card = cache.get(path);

			if (card) return png(card, cacheControl);

			try {
				const drawn = await cache.forceFetch(path, {
					ttl,
					context: () => {
						if (queue.size >= waiting) throw new Busy();

						return queue.add(async () => {
							const started = performance.now();
							const rendered = await draw();
							const ms = Math.round(performance.now() - started);
							log.info({ card: path, ms, bytes: rendered.byteLength }, 'rendered a card');

							return rendered;
						});
					}
				});

				return png(drawn, cacheControl);
			} catch (cause) {
				if (!(cause instanceof Busy)) throw cause;
				log.warn({ card: path, waiting: queue.size }, 'turned away a card: too many waiting to be drawn');

				return new Response('Too many previews being drawn. Try again in a few seconds.', {
					status: 503,
					headers: { 'retry-after': String(RETRY_AFTER), 'cache-control': 'no-store' }
				});
			}
		}
	};
}

export const cards = createCardServer({ budget: CACHE_BYTES, concurrency: RENDERS, waiting: WAITING });
