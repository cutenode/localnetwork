import { setTimeout as sleep } from 'node:timers/promises';
import type { Logger } from '../logger';
import type { DirectoryRecord, HappyViewClient } from './client';
import { unlessOffline } from './offline';

const POLL_MS = 100;

/** How long a write waits for the directory to serve it before answering without it. */
export const INDEX_WAIT_MS = 5_000;

/** The most URIs one getRecords call takes (its lexicon's maxLength). */
const URIS_PER_LOOKUP = 25;

/**
 * `strict` is for reads a write depends on: there a HappyView that's down throws its HappyViewError (which pages show
 * as the directory not answering) instead of reading as nothing, which would look like a record that isn't there.
 */
type ReadOptions = { strict?: boolean };

/**
 * Polls until `served` says the directory serves a write, so the next page load reads it: once, then for up to
 * `waitMs`. Resolves false instead of failing when time runs out or HappyView can't answer: the write happened, and the
 * directory catches up on its own.
 */
export async function awaitIndexed(served: () => Promise<boolean>, waitMs: number): Promise<boolean> {
	const deadline = Date.now() + waitMs;

	for (;;) {
		if (await served().catch(() => false)) return true;

		if (Date.now() >= deadline) return false;
		await sleep(POLL_MS);
	}
}

/**
 * Has HappyView re-read one account's records of `collection` from its PDS, for a write it didn't serve in time, in
 * case Jetstream never brought it. Only with an API key. It waits up to `waitMs` for the job to finish, which then runs
 * on in HappyView; a failure is only logged.
 */
export async function reindex(directory: HappyViewClient, logger: Logger, collection: string, did: string, waitMs = 0) {
	if (!directory.admin) return;

	try {
		const job = await directory.backfill(collection, did);

		if (waitMs) await directory.awaitBackfill(job, waitMs);
	} catch (error) {
		logger.warn({ err: error, collection, did }, 'could not have HappyView re-read a repo');
	}
}

/**
 * Reads public records the way PublicRepo reads them from a PDS, from what HappyView indexed instead
 * (`directory.getRecords` and `directory.listRecords`), so only collections it has a record script for have any. A
 * HappyView that's down reads as having nothing, never as an error (unless the read is `strict`), so a page that shows
 * someone's records still renders; callers parse what comes back.
 */
export class IndexedRepo {
	constructor(
		private readonly directory: HappyViewClient,
		private readonly logger: Logger,
		private readonly indexWaitMs = INDEX_WAIT_MS
	) {}

	async getRecord(
		did: string,
		collection: string,
		rkey: string,
		options: ReadOptions = {}
	): Promise<DirectoryRecord | null> {
		const uri = `at://${did}/${collection}/${rkey}`;

		return (await this.getRecords([uri], options)).get(uri) ?? null;
	}

	/**
	 * A record HappyView may not have yet, since it only reads an account's older records when asked to: when it doesn't,
	 * it re-reads the account's `collection` first, waiting as long as a write would.
	 */
	async getRecordOrReindex(
		did: string,
		collection: string,
		rkey: string,
		options: ReadOptions = {}
	): Promise<DirectoryRecord | null> {
		const found = await this.getRecord(did, collection, rkey, options);

		if (found) return found;
		await this.reindex(did, collection, this.indexWaitMs);

		return this.getRecord(did, collection, rkey, options);
	}

	/** The records HappyView has of `uris`, by URI, in as few lookups as it takes. */
	async getRecords(uris: string[], options: ReadOptions = {}): Promise<Map<string, DirectoryRecord>> {
		const wanted = [...new Set(uris)];
		const lookups = [];

		for (let start = 0; start < wanted.length; start += URIS_PER_LOOKUP)
			lookups.push(
				this.read(options, () =>
					this.directory.query('getRecords', { uris: wanted.slice(start, start + URIS_PER_LOOKUP) })
				)
			);

		const records = (await Promise.all(lookups)).flatMap((lookup) => lookup?.records ?? []);

		return new Map(records.map((record) => [record.uri, record]));
	}

	async listRecords(
		did: string,
		collection: string,
		options: ReadOptions & { limit?: number; cursor?: string } = {}
	): Promise<{ records: DirectoryRecord[]; cursor?: string }> {
		const params = { actor: did, collection, limit: options.limit ?? 100, cursor: options.cursor };

		return (await this.read(options, () => this.directory.query('listRecords', params))) ?? { records: [] };
	}

	/** One account's records of `collection`, a page of 100 at a time, up to `pages` pages. */
	async listAll(
		did: string,
		collection: string,
		options: ReadOptions & { pages?: number } = {}
	): Promise<DirectoryRecord[]> {
		const records: DirectoryRecord[] = [];
		let cursor: string | undefined;

		for (let page = 0; page < (options.pages ?? Infinity); page++) {
			const result = await this.listRecords(did, collection, { strict: options.strict, cursor });
			records.push(...result.records);
			cursor = result.cursor;

			if (!cursor) break;
		}

		return records;
	}

	/**
	 * Waits for HappyView to serve the record as version `cid`, or not at all for null (a delete), so the next page load
	 * reads the write. When it doesn't in time, HappyView re-reads that collection from the account's repo.
	 */
	async awaitWrite(did: string, collection: string, rkey: string, cid: string | null): Promise<void> {
		const uri = `at://${did}/${collection}/${rkey}`;

		const served = async () =>
			((await this.directory.query('getRecords', { uris: [uri] })).records[0]?.cid ?? null) === cid;

		if (await awaitIndexed(served, this.indexWaitMs)) return;
		this.logger.warn({ uri, cid }, 'the directory has not caught up with a write yet');
		await this.reindex(did, collection);
	}

	/** Has HappyView re-read one account's records of `collection` from its PDS, waiting up to `waitMs` (see reindex). */
	reindex(did: string, collection: string, waitMs = 0): Promise<void> {
		return reindex(this.directory, this.logger, collection, did, waitMs);
	}

	private read<T>({ strict }: ReadOptions, query: () => Promise<T>): Promise<T | null> {
		return strict ? query() : unlessOffline(this.logger, query);
	}
}
