import { requestIndex } from '#lib/server/happyview/request-index.ts';
import type { RequestHandler } from './$types';

/** locallibrary asks for its account's records to be read again, with service auth from the account's PDS. */
export const POST: RequestHandler = ({ request, locals: { app, flags, log } }) =>
	requestIndex(request.headers.get('authorization'), {
		audience: app.config.serviceDid,
		getSigningKey: (did, force) => app.identity.signingKey(did, force),
		backfills: app.backfills,
		paused: flags.pauseRequestIndex,
		log
	});
