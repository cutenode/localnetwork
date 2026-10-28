import type { Secp256k1Keypair } from '@atproto/crypto';
import type { JsonObject } from '@atproto/lex-json';

const b64 = (value: JsonObject) => Buffer.from(JSON.stringify(value)).toString('base64url');

/** An `Authorization` header with an inter-service JWT `keypair` signed, as a PDS's getServiceAuth mints them. */
export async function serviceJwt(keypair: Secp256k1Keypair, payload: JsonObject, header: JsonObject = {}) {
	const input = `${b64({ alg: 'ES256K', typ: 'JWT', ...header })}.${b64(payload)}`;
	const signature = await keypair.sign(new TextEncoder().encode(input));

	return `Bearer ${input}.${Buffer.from(signature).toString('base64url')}`;
}
