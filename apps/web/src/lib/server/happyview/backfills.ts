import type { Logger } from '../logger';
import type { HappyViewClient } from './client';

/** How long a caller waits for HappyView to read an account's records before going on without them. */
const WAIT_MS = 5_000;

/** How often one account's records of one collection are re-read at most. Jetstream keeps them current in between. */
const EVERY_MS = 10 * 60_000;

/**
 * Has HappyView re-read one account's records of a collection from its PDS, for records Jetstream never brought (ones
 * written before HappyView kept that collection, or while it was down). Only with an API key, and at most every ten
 * minutes per account and collection: a caller that comes while a re-read runs, or soon after, gets that re-read's
 * outcome instead of crawling the repo again. A failed one is forgotten, so the next caller tries again.
 */
export class Backfills {
	private readonly last = new Map<string, { at: number; done: Promise<boolean> }>();

	constructor(
		private readonly directory: HappyViewClient,
		private readonly logger: Logger
	) {}

	/**
	 * Whether HappyView finished re-reading within five seconds: false without an API key, after a failure, or while it
	 * carries on reading.
	 */
	async backfill(collection: string, did: string): Promise<boolean> {
		if (!this.directory.admin) return false;
		const key = `${collection} ${did}`;
		const last = this.last.get(key);

		if (last && Date.now() - last.at < EVERY_MS) return last.done;
		const done = this.read(key, collection, did);
		this.last.set(key, { at: Date.now(), done });

		return done;
	}

	private async read(key: string, collection: string, did: string): Promise<boolean> {
		try {
			const job = await this.directory.backfill(collection, did);
			const status = await this.directory.awaitBackfill(job, WAIT_MS);

			if (status === 'failed' || status === 'cancelled') throw new Error(`HappyView's re-read ${status}`);

			if (status === 'completed') return true;
			this.logger.info({ collection, did, status }, 'HappyView is still re-reading a repo');

			return false;
		} catch (error) {
			this.last.delete(key);
			this.logger.warn({ err: error, collection, did }, 'could not have HappyView re-read a repo');

			return false;
		}
	}
}
