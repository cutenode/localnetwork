import { P256_JWT_ALG, SECP256K1_JWT_ALG } from '@atproto/crypto';
import { AuthRequiredError, cryptoVerifySignatureWithKey, verifyJwt } from '@atproto/xrpc-server';

export class ServiceAuthError extends Error {
	constructor(message: string) {
		super(message);
		this.name = 'ServiceAuthError';
	}
}

/** The algs atproto keys sign with. */
const SIGNING_ALGS = new Set([P256_JWT_ALG, SECP256K1_JWT_ALG]);

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
		payload = await verifyJwt(
			token,
			options.audience,
			options.lxm,
			(iss, forceRefresh) => options.getSigningKey(issuerDid(iss), forceRefresh),
			async (key, msgBytes, sigBytes, alg) => {
				// @atproto/crypto only checks alg against the key when alg is truthy, so "" would verify with any key's alg.
				if (!SIGNING_ALGS.has(alg)) throw new Error(`Unsupported jwt alg "${alg}"`);

				return cryptoVerifySignatureWithKey(key, msgBytes, sigBytes, alg);
			}
		);
	} catch (error) {
		// A part that isn't base64url JSON throws JSON.parse's error, not one of verifyJwt's.
		throw new ServiceAuthError(error instanceof AuthRequiredError ? error.message : 'Malformed service auth token');
	}

	return { ...payload, iss: issuerDid(payload.iss) };
}
