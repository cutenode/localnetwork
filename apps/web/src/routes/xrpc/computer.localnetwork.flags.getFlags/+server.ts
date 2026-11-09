import { getFlags } from '#lib/server/flags/get-flags.ts';
import { clientIp } from '#lib/server/http.ts';
import type { RequestHandler } from './$types';

/** Which flags are on for a caller: locallibrary, with service auth from the account's PDS, or anyone without it. */
export const GET: RequestHandler = (event) => {
	const { app, log } = event.locals;

	return getFlags(event.request.headers.get('authorization'), {
		audience: app.config.serviceDid,
		getSigningKey: (did, force) => app.identity.signingKey(did, force),
		flags: app.flags,
		limit: app.limits.flags,
		clientIp: clientIp(event),
		log
	});
};
