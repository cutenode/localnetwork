import { isDeepStrictEqual } from 'node:util';
import { COLLECTION } from '#lib/nsid.ts';
import type { HappyViewBundle } from './bundle';
import type { HappyViewClient } from './client';

/** How long a sync waits for HappyView to finish re-reading the root's projects. */
const BACKFILL_WAIT_MS = 30_000;

/**
 * Brings HappyView in step with this app: uploads the scripts and lexicons that differ from what it
 * stores (an upload bumps a lexicon's revision even when nothing changed), then has it re-read the
 * root account's projects and waits for that, so the directory is complete when this resolves.
 * Safe to run on every boot. Returns the ids it uploaded.
 */
export async function syncHappyView(client: HappyViewClient, bundle: HappyViewBundle, rootDid: string) {
	const [storedLexicons, storedScripts] = await Promise.all([
		Promise.all(bundle.lexicons.map((doc) => client.lexicon(doc.id))),
		client.scripts()
	]);

	// HappyView hands documents back with its own key order, so they're compared as values.
	const lexicons = bundle.lexicons.filter((doc, index) => !isDeepStrictEqual(storedLexicons[index], doc));

	const bodies = new Map(storedScripts.map((script) => [script.id, script.body]));
	const scripts = bundle.scripts.filter((script) => bodies.get(script.id) !== script.body);

	// Scripts go first. A record lexicon starts ingestion, and until its record script exists HappyView
	// would store every account's records of that collection.
	await Promise.all(scripts.map((script) => client.putScript(script)));

	// One at a time, in bundle order, so the shared defs are stored before the queries that use them.
	for (const doc of lexicons) await client.putLexicon(doc);

	const status = await client.awaitBackfill(await client.backfill(COLLECTION.project, rootDid), BACKFILL_WAIT_MS);

	if (status === 'failed' || status === 'cancelled') throw new Error(`HappyView's backfill of ${rootDid} ${status}`);

	return { lexicons: lexicons.map((doc) => doc.id), scripts: scripts.map((script) => script.id) };
}
