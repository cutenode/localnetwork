import { GET_FLAGS } from '#lib/nsid.ts';
import { verifyServiceJwt } from '../atproto/service-auth';
import type { Logger } from '../logger';
import type { RateLimiter } from '../rate-limit';
import type { FlagService } from '../services/flags';

type GetFlagsOptions = {
	/** This app's did:web, which the caller's PDS signs the token for. */
	audience: string;
	getSigningKey: (did: string, forceRefresh: boolean) => Promise<string>;
	flags: Pick<FlagService, 'answers'>;
	/** Checked for tokens only, since checking one can mean resolving someone else's DID. */
	limit: Pick<RateLimiter, 'take'>;
	clientIp: string;
	log: Logger;
};

const VARY = { vary: 'authorization' };

/**
 * Answers `computer.localnetwork.flags.getFlags`. The staff space is private, so this is how locallibrary learns what
 * the website already knows. With no token it says what's true of every caller. With one, it also says which flags
 * target the token's issuer, and never who else they target.
 */
export async function getFlags(authorization: string | null, options: GetFlagsOptions): Promise<Response> {
	const { audience, getSigningKey, flags, limit, clientIp, log } = options;

	if (!authorization) {
		return Response.json(
			{ flags: await flags.answers(null) },
			{ headers: { ...VARY, 'cache-control': 'public, max-age=60' } }
		);
	}

	if (!limit.take(`ip:${clientIp}`)) {
		return Response.json(
			{ error: 'RateLimitExceeded', message: 'Slow down a little' },
			{ status: 429, headers: { 'retry-after': '30' } }
		);
	}

	let did: string;

	try {
		({ iss: did } = await verifyServiceJwt(authorization, { audience, lxm: GET_FLAGS, getSigningKey }));
	} catch (error) {
		log.warn('rejected getFlags', { err: error });

		return Response.json(
			{ error: 'AuthenticationRequired', message: 'Invalid service auth' },
			{ status: 401, headers: VARY }
		);
	}

	return Response.json(
		{ flags: await flags.answers(did) },
		{ headers: { ...VARY, 'cache-control': 'private, no-store' } }
	);
}
