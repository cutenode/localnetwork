import { createHash } from 'node:crypto';
import { AtUri } from '@atproto/syntax';
import { USERINPUT } from '#lib/bug-report.ts';
import { BSKY_FOLLOW, BSKY_PROFILE, CALENDAR_EVENT, COLLECTION, VERIFICATION } from '#lib/nsid.ts';
import type { Core } from '../core';
import type { HappyViewClient } from './client';

/**
 * The collections HappyView keeps that the site reads per project manager (their Local Network profile, and their
 * Bluesky profile for verification statuses). Jetstream only brings records written after HappyView started keeping them, so each manager's older ones
 * are re-read: once per version of the collection's record script, and when they become a manager.
 */
export const MANAGER_COLLECTIONS = [
	COLLECTION.djSet,
	COLLECTION.djRecording,
	COLLECTION.djProfile,
	COLLECTION.epk,
	CALENDAR_EVENT,
	COLLECTION.profile,
	BSKY_PROFILE
];

/** A collection HappyView re-reads after a new record script, and the accounts it re-reads it from. */
export type RefreshTarget = { collection: string; accounts: () => Promise<string[]> };

/**
 * What the app has HappyView re-read for older records: each of MANAGER_COLLECTIONS from the root and every manager
 * (`managers`, asked once), the follows `followReaders` names, the root's verifications, DJ listings and listings, and
 * the bug report space from its owner when there is one.
 */
export function refreshTargets(
	rootDid: string,
	sources: { managers: () => Promise<string[]>; followReaders: () => Promise<string[]>; userinputSpace?: string }
): RefreshTarget[] {
	let managers: Promise<string[]> | undefined;
	const root = async () => [rootDid];
	const spaceOwner = sources.userinputSpace && new AtUri(sources.userinputSpace).host;

	return [
		...MANAGER_COLLECTIONS.map((collection) => ({
			collection,
			accounts: () => (managers ??= sources.managers().then((dids) => [rootDid, ...dids]))
		})),
		{ collection: BSKY_FOLLOW, accounts: sources.followReaders },
		{ collection: VERIFICATION, accounts: root },
		{ collection: COLLECTION.listingProfile, accounts: root },
		{ collection: COLLECTION.listing, accounts: root },
		...(spaceOwner ? [{ collection: USERINPUT.space, accounts: async () => [spaceOwner] }] : [])
	];
}

/**
 * Has HappyView re-read `collection` from each account `accounts` lists, once per version of its record script
 * (`script`), which only applies to records HappyView reads after it. The version is kept in the app's database once
 * every re-read has started, so a failure or a restart before then tries again, and a later boot doesn't. `uploaded` says
 * HappyView was just given the script, so it needs the re-reads even if this app made them for the same version before.
 * Starts one job per 500 accounts and doesn't wait for them to finish.
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
	core.logger.info('having HappyView re-read a collection for a new script', { collection, accounts: dids.length });

	try {
		await directory.backfill(collection, dids);
	} catch (error) {
		core.logger.warn('could not have HappyView re-read a collection', {
			err: error,
			collection,
			accounts: dids.length
		});

		return;
	}

	await core.db
		.insertInto('kv')
		.values({ key, value: version })
		.onConflict((conflict) => conflict.column('key').doUpdateSet({ value: version }))
		.execute();
}
