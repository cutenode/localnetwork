import { Secp256k1Keypair } from '@atproto/crypto';
import { describe, expect, it, vi } from 'vitest';
import { silentLogger } from '../logger';
import { type DidDocument, IdentityResolver, isPrivateAddress } from './identity';

describe('isPrivateAddress', () => {
	it.each([
		'127.0.0.1',
		'10.1.2.3',
		'172.16.0.1',
		'192.168.1.1',
		'169.254.1.1',
		'100.64.0.1',
		'0.0.0.0',
		'::1',
		'fc00::1',
		'fe80::1',
		'::ffff:127.0.0.1'
	])('flags %s', (address) => {
		expect(isPrivateAddress(address)).toBe(true);
	});
	it.each(['8.8.8.8', '1.1.1.1', '2606:4700::1111', '172.32.0.1'])('allows %s', (address) => {
		expect(isPrivateAddress(address)).toBe(false);
	});
});

describe('IdentityResolver.assertSafeEndpoint', () => {
	const strict = new IdentityResolver({
		plcUrl: 'https://plc.directory',
		allowPrivateNetwork: false,
		logger: silentLogger
	});

	it('refuses plain HTTP and private addresses', async () => {
		await expect(strict.assertSafeEndpoint('http://example.com')).rejects.toThrow(/non-HTTPS/);
		await expect(strict.assertSafeEndpoint('https://127.0.0.1:8080')).rejects.toThrow(/private/);
		await expect(strict.assertSafeEndpoint('https://[::1]')).rejects.toThrow(/private/);
	});
	it('allows anything in development mode', async () => {
		const dev = new IdentityResolver({
			plcUrl: 'https://plc.directory',
			allowPrivateNetwork: true,
			logger: silentLogger
		});

		await expect(dev.assertSafeEndpoint('http://localhost:2583')).resolves.toBeUndefined();
	});
	it('resolves DIDs as identifiers without network access', async () => {
		await expect(strict.resolveIdentifier(' did:plc:abc ')).resolves.toBe('did:plc:abc');
	});
});

describe('IdentityResolver.resolve', () => {
	const did = 'did:plc:mallory';

	async function resolveClaiming(handleOwner: string | null) {
		const key = (await Secp256k1Keypair.create()).did().slice('did:key:'.length);

		const doc: DidDocument = {
			id: did,
			alsoKnownAs: ['at://alice.example.com'],
			verificationMethod: [{ id: `${did}#atproto`, type: 'Multikey', controller: did, publicKeyMultibase: key }],
			service: [{ id: '#atproto_pds', type: 'AtprotoPersonalDataServer', serviceEndpoint: 'https://pds.example.com' }]
		};

		const identity = new IdentityResolver({
			plcUrl: 'https://plc.directory',
			allowPrivateNetwork: false,
			logger: silentLogger
		});

		vi.spyOn(identity.resolver.did, 'resolve').mockResolvedValue(doc);
		vi.spyOn(identity.resolver.handle, 'resolve').mockResolvedValue(handleOwner ?? undefined);

		return identity.resolve(did);
	}

	it('returns a handle that resolves back to the DID', async () => {
		expect((await resolveClaiming(did)).handle).toBe('alice.example.com');
	});

	it('drops a handle the DID claims but does not control', async () => {
		expect((await resolveClaiming('did:plc:alice')).handle).toBeNull();
		expect((await resolveClaiming(null)).handle).toBeNull();
	});
});
