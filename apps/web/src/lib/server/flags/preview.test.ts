import fc from 'fast-check';
import { describe, expect, it } from 'vitest';
import { newToken } from '../auth/session';
import { openPreview, PREVIEW_MS, sealPreview } from './preview';

const DID = 'did:plc:ada';

const TOKEN = newToken();

const NOW = Date.parse('2026-11-09T12:00:00.000Z');

const sealed = (flags: Record<string, boolean>, did = DID) =>
	sealPreview({ did, flags, expiresAt: NOW + PREVIEW_MS }, TOKEN);

describe('a staff preview cookie', () => {
	it('opens for the account and session that made it, until it expires', () => {
		const value = sealed({ parityCanary: true });

		expect(openPreview(value, TOKEN, DID, NOW)).toEqual({ parityCanary: true });
		expect(openPreview(value, TOKEN, DID, NOW + PREVIEW_MS - 1)).toEqual({ parityCanary: true });
		expect(openPreview(value, TOKEN, DID, NOW + PREVIEW_MS)).toBeNull();
	});

	it('does nothing for another account, another session or no session', () => {
		const value = sealed({ parityCanary: true });

		expect(openPreview(value, TOKEN, 'did:plc:milo', NOW)).toBeNull();
		expect(openPreview(value, newToken(), DID, NOW)).toBeNull();
		expect(openPreview(value, undefined, DID, NOW)).toBeNull();
	});

	it('refuses a value anyone changed or made up', () => {
		const value = sealed({ parityCanary: false });
		const [payload, signature] = value.split('.');
		const forged = Buffer.from(JSON.stringify({ did: DID, flags: { parityCanary: true }, expiresAt: NOW + 1 }));

		expect(openPreview(`${forged.toString('base64url')}.${signature}`, TOKEN, DID, NOW)).toBeNull();
		expect(openPreview(`${payload}.`, TOKEN, DID, NOW)).toBeNull();
		expect(openPreview(payload, TOKEN, DID, NOW)).toBeNull();
		expect(openPreview('', TOKEN, DID, NOW)).toBeNull();
		expect(openPreview(undefined, TOKEN, DID, NOW)).toBeNull();
	});

	it('drops flags that have left the catalog', () => {
		expect(openPreview(sealed({ parityCanary: true, retired: true }), TOKEN, DID, NOW)).toEqual({ parityCanary: true });
	});

	it('never opens with a signature it did not make', () => {
		fc.assert(
			fc.property(fc.string(), fc.string(), (payload, signature) => {
				expect(openPreview(`${payload}.${signature}`, TOKEN, DID, NOW)).toBeNull();
			})
		);
	});
});
