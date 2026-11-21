import type { Logger } from '../logger';
import type { FlagService } from '../services/flags';
import { BACKFILL_FAILED, type HappyViewClient } from './client';

/** How long a caller waits for HappyView to read an account's records before going on without them. */
const WAIT_MS = 5_000;

/** How often one account's records of one collection are re-read at most. Jetstream keeps them current in between. */
const EVERY_MS = 10 * 60_000;

/** The app's flags, for pauseBackfills. A caller without them never pauses its re-reads. */
export type BackfillFlags = Pick<FlagService, 'evaluate'> | undefined;

/**
 * Whether the pauseBackfills flag, on for everyone, stops a re-read of `collection` for `dids` now. It notes the skip,
 * so staff can see the switch working.
 */
export async function backfillsPaused(
	flags: BackfillFlags,
	logger: Logger,
	collection: string,
	dids: string[]
): Promise<boolean> {
	const paused = !!(await flags?.evaluate(null))?.pauseBackfills;

	if (paused) logger.info('HappyView re-reads paused', { collection, accounts: dids.length, flag: 'pauseBackfills' });

	return paused;
}

/**
 * Has HappyView re-read accounts' records of a collection from their PDS, for records Jetstream never brought (ones
 * written before HappyView kept that collection, or while it was down). Only with an API key, and at most every ten
 * minutes per account and collection: a caller that comes while a re-read runs, or soon after, gets that re-read's
 * outcome instead of crawling the repo again. A failed one is forgotten, so the next caller tries again. The pauseBackfills
 * flag, on for everyone, stops new re-reads without a deploy.
 */
export class Backfills {
	private readonly last = new Map<string, { at: number; done: Promise<boolean> }>();

	constructor(
		private readonly directory: HappyViewClient,
		private readonly logger: Logger,
		private readonly flags?: BackfillFlags
	) {}

	/**
	 * Whether HappyView finished re-reading every account in `dids` within five seconds: false without an API key, while
	 * paused, after a failure, or while it carries on reading. The accounts not read lately are read together, in one job per 500.
	 */
	async backfill(collection: string, dids: string[]): Promise<boolean> {
		if (!this.directory.admin) return false;

		if (await backfillsPaused(this.flags, this.logger, collection, dids)) return false;

		const reads: Promise<boolean>[] = [];
		const due: string[] = [];

		for (const did of new Set(dids)) {
			const last = this.last.get(`${collection} ${did}`);

			if (last && Date.now() - last.at < EVERY_MS) reads.push(last.done);
			else due.push(did);
		}

		if (due.length) {
			const done = this.read(collection, due);
			reads.push(done);

			for (const did of due) this.last.set(`${collection} ${did}`, { at: Date.now(), done });
		}

		return (await Promise.all(reads)).every(Boolean);
	}

	private async read(collection: string, dids: string[]): Promise<boolean> {
		try {
			const jobs = await this.directory.backfill(collection, dids);
			const statuses = await Promise.all(jobs.map((job) => this.directory.awaitBackfill(job, WAIT_MS)));
			const failed = statuses.find((status) => BACKFILL_FAILED.has(status ?? ''));

			if (failed) throw new Error(`HappyView's re-read ${failed}`);

			if (statuses.every((status) => status === 'completed')) return true;
			this.logger.info('HappyView is still re-reading repos', { collection, dids, backfillStatus: statuses });

			return false;
		} catch (error) {
			for (const did of dids) this.last.delete(`${collection} ${did}`);
			this.logger.warn('could not have HappyView re-read repos', { err: error, collection, dids });

			return false;
		}
	}
}
