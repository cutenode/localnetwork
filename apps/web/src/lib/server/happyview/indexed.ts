import { setTimeout as sleep } from 'node:timers/promises';
import type { Logger } from '../logger';
import type { HappyViewClient } from './client';

const POLL_MS = 100;

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
 * case Jetstream never brought it. Only with an API key; the job runs on in HappyView, and a failure is only logged.
 */
export async function reindex(directory: HappyViewClient, logger: Logger, collection: string, did: string) {
	if (!directory.admin) return;

	try {
		await directory.backfill(collection, did);
	} catch (error) {
		logger.warn({ err: error, collection, did }, 'could not have HappyView re-read a repo');
	}
}
