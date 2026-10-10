import { verifySignature } from '@atproto/crypto';
import { z } from 'zod';

export class ServiceAuthError extends Error {
	constructor(message: string) {
		super(message);
		this.name = 'ServiceAuthError';
	}
}

const serviceJwtHeader = z.object({ alg: z.string(), typ: z.string().optional() });

const serviceJwtPayload = z.object({
	iss: z.string(),
	aud: z.string(),
	exp: z.number(),
	iat: z.number().optional(),
	lxm: z.string().optional(),
	jti: z.string().optional()
});

export type ServiceJwtPayload = z.infer<typeof serviceJwtPayload>;

const decode = <T>(schema: z.ZodType<T>, part: string): T =>
	schema.parse(JSON.parse(Buffer.from(part, 'base64url').toString('utf8')));

/**
 * Verifies an inbound atproto inter-service JWT (the `Authorization: Bearer`
 * a space host sends with `notifyWrite`). Checks audience, method binding
 * (`lxm`), expiry, and the signature against the issuer's atproto key,
 * refreshing the key once in case it rotated.
 */
export async function verifyServiceJwt(
	authorization: string | null | undefined,
	options: {
		audience: string;
		lxm: string;
		getSigningKey: (did: string, forceRefresh: boolean) => Promise<string>;
		now?: number;
	}
): Promise<ServiceJwtPayload> {
	const token = authorization?.match(/^Bearer\s+(\S+)$/i)?.[1];

	if (!token) throw new ServiceAuthError('Missing service auth token');
	const parts = token.split('.');

	if (parts.length !== 3) throw new ServiceAuthError('Malformed service auth token');
	const [headerPart, payloadPart, signaturePart] = parts;

	let header: z.infer<typeof serviceJwtHeader>;
	let payload: ServiceJwtPayload;

	try {
		header = decode(serviceJwtHeader, headerPart);
		payload = decode(serviceJwtPayload, payloadPart);
	} catch {
		throw new ServiceAuthError('Malformed service auth token');
	}

	if (header.typ === 'at+jwt' || header.typ === 'refresh+jwt' || header.typ === 'dpop+jwt') {
		throw new ServiceAuthError('Not a service auth token');
	}

	const now = Math.floor((options.now ?? Date.now()) / 1000);

	if (payload.exp < now) throw new ServiceAuthError('Service auth token expired');

	if (payload.aud !== options.audience) throw new ServiceAuthError('Service auth audience mismatch');

	if (payload.lxm !== undefined && payload.lxm !== options.lxm)
		throw new ServiceAuthError('Service auth method mismatch');

	if (!payload.iss.startsWith('did:')) throw new ServiceAuthError('Bad issuer');

	const issuerDid = payload.iss.split('#')[0];
	const signingInput = new TextEncoder().encode(`${headerPart}.${payloadPart}`);
	const signature = new Uint8Array(Buffer.from(signaturePart, 'base64url'));

	const check = async (forceRefresh: boolean) => {
		const key = await options.getSigningKey(issuerDid, forceRefresh);

		try {
			return await verifySignature(key, signingInput, signature, {
				jwtAlg: header.alg,
				allowMalleableSig: true
			});
		} catch {
			return false;
		}
	};

	if (!(await check(false)) && !(await check(true))) throw new ServiceAuthError('Invalid service auth signature');

	return { ...payload, iss: issuerDid };
}
