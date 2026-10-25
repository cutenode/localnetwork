import { beforeEach, describe, expect, it } from 'vitest';
import { silentLogger } from '../logger';
import { FakeSpaceHost } from '../testing/space-host';
import { SpaceCredentialManager } from './credentials';
import { bookingSpace } from './spaces';

const SPACE = bookingSpace('did:plc:root', 'gig');

const DJ = 'did:plc:dj';

const NOTE = 'computer.localnetwork.test.note';

const listRepos = { nsid: 'com.atproto.space.listRepos', params: { space: SPACE }, audience: 'did:plc:root' };

describe('SpaceCredentialManager', () => {
	let host: FakeSpaceHost;
	let credentials: SpaceCredentialManager;

	beforeEach(async () => {
		host = await FakeSpaceHost.create(SPACE);

		credentials = new SpaceCredentialManager({
			source: async () => ({ getDelegationToken: async () => 'delegation-token' }),
			identity: host.identity,
			logger: silentLogger,
			fetch: host.fetch
		});
	});

	it('asks for a credential with the delegation token in the body, then signs requests with it', async () => {
		const response = await credentials.request({ space: SPACE, service: host.url, ...listRepos });
		expect(response.ok).toBe(true);

		const [ask, use] = host.requests;
		expect(ask.nsid).toBe('com.atproto.space.getSpaceCredential');
		expect(ask.body).toEqual({
			space: SPACE,
			credentialKid: expect.stringMatching(/^did:key:zDn/),
			delegationToken: 'delegation-token'
		});
		expect(ask.headers.get('authorization')).toBeNull();
		expect(ask.headers.get('signature')).toBeNull();
		expect(use.headers.get('authorization')).toMatch(/^Atproto-Space-Credential /);
		expect(use.headers.get('signature-input')).toContain('tag="atproto-space-credential"');
	});

	it('gets a fresh credential and tries once more when the host revoked the one it had', async () => {
		await credentials.request({ space: SPACE, service: host.url, ...listRepos });
		host.revoked.add(host.issued[0]);

		const response = await credentials.request({ space: SPACE, service: host.url, ...listRepos });
		expect(response.ok).toBe(true);
		expect(host.issued).toHaveLength(2);
	});

	it('reads a record checked against its MAC, and null for one that is not there', async () => {
		await host.put(DJ, NOTE, 'a', { $type: NOTE, text: 'hi' });
		const record = { space: SPACE, repo: DJ, collection: NOTE };

		await expect(credentials.readRecord({ ...record, rkey: 'a' })).resolves.toMatchObject({
			uri: `${SPACE}/${DJ}/${NOTE}/a`,
			value: { $type: NOTE, text: 'hi' }
		});
		await expect(credentials.readRecord({ ...record, rkey: 'b' })).resolves.toBeNull();
	});

	it('refuses a record whose MAC does not verify, after once refreshing the KEM key', async () => {
		await host.put(DJ, NOTE, 'a', { $type: NOTE, text: 'hi' });
		host.forgeMacs = true;

		await expect(credentials.readRecord({ space: SPACE, repo: DJ, collection: NOTE, rkey: 'a' })).rejects.toThrow(
			'does not match its MAC'
		);
		expect(host.identity.refreshed).toEqual([DJ]);
	});
});
