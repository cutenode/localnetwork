import { AuthRequiredError, verifyJwt } from '@atproto/xrpc-server';

export class ServiceAuthError extends Error {
	constructor(message: string) {
		super(message);
		this.name = 'ServiceAuthError';
	}
}

type VerifiedJwt = Awaited<ReturnType<typeof verifyJwt>>;

export type ServiceJwtPayload = Omit<VerifiedJwt, 'iss'> & { iss: string };

/**
 * Verifies an inbound atproto inter-service JWT (the `Authorization: Bearer`
 * a space host sends with `notifyWrite`) with the reference implementation
 * from @atproto/xrpc-server: audience, method binding (`lxm`, which has to be
 * there), expiry, and the signature against the issuer's atproto key,
 * refreshing the key once in case it rotated. The issuer comes back without
 * its `#service` fragment.
 */
export async function verifyServiceJwt(
	authorization: string | null | undefined,
	options: {
		audience: string;
		lxm: string;
		getSigningKey: (did: string, forceRefresh: boolean) => Promise<string>;
	}
): Promise<ServiceJwtPayload> {
	const token = authorization?.match(/^Bearer\s+(\S+)$/i)?.[1];

	if (!token) throw new ServiceAuthError('Missing service auth token');

	const issuerDid = (iss: string) => iss.split('#')[0];
	let payload: VerifiedJwt;

	try {
		payload = await verifyJwt(token, options.audience, options.lxm, (iss, forceRefresh) =>
			options.getSigningKey(issuerDid(iss), forceRefresh)
		);
	} catch (error) {
		// A part that isn't base64url JSON throws JSON.parse's error, not one of verifyJwt's.
		throw new ServiceAuthError(error instanceof AuthRequiredError ? error.message : 'Malformed service auth token');
	}

	return { ...payload, iss: issuerDid(payload.iss) };
}
