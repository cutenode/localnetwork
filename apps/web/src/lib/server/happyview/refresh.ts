import { createHash } from 'node:crypto';
import PQueue from 'p-queue';
import { BSKY_PROFILE, CALENDAR_EVENT, COLLECTION } from '#lib/nsid.ts';
import type { Core } from '../core';
import type { HappyViewClient } from './client';

/**
 * The collections HappyView keeps that the site reads per project manager (their Bluesky profile for verification
 * statuses). Jetstream only brings records written after HappyView started keeping them, so each manager's older ones
 * are re-read: once per version of the collection's record script, and when they become a manager.
 */
export const MANAGER_COLLECTIONS = [
	COLLECTION.djSet,
	COLLECTION.djRecording,
	COLLECTION.djProfile,
	COLLECTION.epk,
	CALENDAR_EVENT,
	BSKY_PROFILE
];

/** How many accounts' re-reads start at once after a new record script. */
const CONCURRENCY = 4;

/**
 * Has HappyView re-read `collection` from each account `accounts` lists, once per version of its record script
 * (`script`), which only applies to records HappyView reads after it. The version is kept in the app's database once
 * every re-read has started, so a failure or a restart before then tries again, and a later boot doesn't. `uploaded` says
 * HappyView was just given the script, so it needs the re-reads even if this app made them for the same version before.
 * Starts a few at a time and doesn't wait for them to finish.
 */
export async function refreshForScript(
	core: Pick<Core, 'db' | 'logger'>,
	directory: HappyViewClient,
	{
		collection,
		script,
		uploaded,
		accounts
	}: { collection: string; script: string; uploaded: boolean; accounts: () => Promise<string[]> }
): Promise<void> {
	const key = `happyview_refreshed:${collection}`;
	const version = createHash('sha256').update(script).digest('hex');

	if (uploaded) await core.db.deleteFrom('kv').where('key', '=', key).execute();
	const done = await core.db.selectFrom('kv').select('value').where('key', '=', key).executeTakeFirst();

	if (done?.value === version) return;
	const dids = await accounts();
	const queue = new PQueue({ concurrency: CONCURRENCY });
	core.logger.info({ collection, accounts: dids.length }, 'having HappyView re-read a collection for a new script');

	const started = await Promise.all(
		dids.map((did) =>
			queue.add(async () => {
				try {
					await directory.backfill(collection, did);

					return true;
				} catch (error) {
					core.logger.warn({ err: error, collection, did }, 'could not have HappyView re-read a repo');

					return false;
				}
			})
		)
	);

	if (!started.every(Boolean)) return;
	await core.db
		.insertInto('kv')
		.values({ key, value: version })
		.onConflict((conflict) => conflict.column('key').doUpdateSet({ value: version }))
		.execute();
}
