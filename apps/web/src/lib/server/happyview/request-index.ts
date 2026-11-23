import { COLLECTION, REQUEST_INDEX } from '#lib/nsid.ts';
import { verifyServiceJwt } from '../atproto/service-auth';
import type { Logger } from '../logger';
import type { Backfills } from './backfills';

/** What locallibrary reads back from HappyView before it replaces it. */
const LOCALLIBRARY_COLLECTIONS = [
	COLLECTION.djSet,
	COLLECTION.djRecording,
	COLLECTION.djProfile,
	COLLECTION.locallibrarySettings
];

type RequestIndexOptions = {
	/** This app's did:web, which the caller's PDS signs the token for. */
	audience: string;
	getSigningKey: (did: string, forceRefresh: boolean) => Promise<string>;
	backfills: Backfills;
	/** pauseRequestIndex: the answer is that nothing was indexed, as without an API key, and HappyView is left alone. */
	paused: boolean;
	log: Logger;
};

/**
 * Answers `computer.localnetwork.directory.requestIndex`: has HappyView re-read the token issuer's locallibrary
 * records, and only theirs, since locallibrary needs the ones Jetstream never brought before it replaces them.
 */
export async function requestIndex(authorization: string | null, options: RequestIndexOptions): Promise<Response> {
	const { audience, getSigningKey, backfills, paused, log } = options;
	let did: string;

	try {
		({ iss: did } = await verifyServiceJwt(authorization, { audience, lxm: REQUEST_INDEX, getSigningKey }));
	} catch (error) {
		log.warn('rejected requestIndex', { err: error });

		return Response.json({ error: 'AuthenticationRequired', message: 'Invalid service auth' }, { status: 401 });
	}

	if (paused) {
		log.info('requestIndex paused', { did, flag: 'pauseRequestIndex' });

		return Response.json({ indexed: false });
	}

	const done = await Promise.all(LOCALLIBRARY_COLLECTIONS.map((collection) => backfills.backfill(collection, [did])));
	const indexed = done.every(Boolean);
	log.info("had HappyView re-read an account's locallibrary records", { did, indexed });

	return Response.json({ indexed });
}
