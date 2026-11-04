import { beforeEach, describe, expect, it, vi } from 'vitest';
import { z } from 'zod';
import { silentLogger } from '../logger';
import { type Loopback, rebindToLoopback } from '../testing/rebind';
import { FakeSpaceHost } from '../testing/space-host';
import { SpaceCredentialManager } from './credentials';
import { IdentityResolver } from './identity';
import { RecordValidationError } from './lexicons';
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
			logger: silentLogger
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

	it('refuses a record nested deeper than 32 levels as invalid', async () => {
		// The record is the first level, so 32 nested arrays in a field make 33.
		const extra = JSON.parse(`${'['.repeat(32)}0${']'.repeat(32)}`);
		await host.put(DJ, NOTE, 'a', { $type: NOTE, extra });

		await expect(credentials.readRecord({ space: SPACE, repo: DJ, collection: NOTE, rkey: 'a' })).rejects.toThrow(
			new RecordValidationError(NOTE, 'nested deeper than 32 levels')
		);
	});

	it('refuses a record whose MAC does not verify, after once refreshing the KEM key', async () => {
		await host.put(DJ, NOTE, 'a', { $type: NOTE, text: 'hi' });
		host.forgeMacs = true;

		await expect(credentials.readRecord({ space: SPACE, repo: DJ, collection: NOTE, rkey: 'a' })).rejects.toThrow(
			'does not match its MAC'
		);
		expect(host.identity.refreshed).toEqual([DJ]);
	});

	it('mints one credential for requests that need it at once, and only for a space URI', async () => {
		await Promise.all([credentials.get(SPACE), credentials.get(SPACE)]);
		expect(host.issued).toHaveLength(1);

		await expect(credentials.get('at://did:plc:root/app.bsky.feed.post/1')).rejects.toThrow(
			'Not a space URI: at://did:plc:root/app.bsky.feed.post/1'
		);
	});

	it('signs requests only for an audience that is a DID', async () => {
		const credential = await credentials.get(SPACE);

		await expect(credential.headers('alice.example.com')).rejects.toThrow('Space audience is not a DID');
	});

	it('explains a delegation token refused for an app password, and passes any other failure on', async () => {
		const minting = (failure: Error | string) =>
			new SpaceCredentialManager({
				source: async () => ({
					getDelegationToken: async () => {
						throw failure;
					}
				}),
				identity: host.identity,
				logger: silentLogger
			}).get(SPACE);

		await expect(minting(new Error('InvalidToken: Bad token scope'))).rejects.toThrow(
			/ROOT_PASSWORD looks like an app password/
		);
		await expect(minting(new Error('fetch failed'))).rejects.toThrow('fetch failed');
		await expect(minting('Bad token scope')).rejects.toBe('Bad token scope');
	});

	it('refuses a credential response without a credential', async () => {
		vi.spyOn(host, 'fetch').mockResolvedValueOnce(Response.json({}));

		await expect(credentials.get(SPACE)).rejects.toMatchObject({
			status: 502,
			error: 'InvalidResponse',
			message: 'InvalidResponse: No credential returned'
		});
	});

	it("calls a procedure with the space's credential, retrying once with a fresh one if it was revoked", async () => {
		const procedure = <T>(schema: z.ZodType<T>) =>
			credentials.procedure({ space: SPACE, service: host.url, ...listRepos, body: {}, schema });

		await host.put(DJ, NOTE, 'a', { $type: NOTE, text: 'hi' });
		const repos = z.object({ repos: z.array(z.object({ did: z.string() })) });

		expect(await procedure(repos)).toEqual({ repos: [{ did: DJ }] });
		expect(host.requests.at(-1)?.body).toEqual({});

		host.revoked.add(host.issued[0]);
		expect(await procedure(repos)).toEqual({ repos: [{ did: DJ }] });
		expect(host.issued).toHaveLength(2);

		// A procedure that answers with no body reads as an empty object.
		vi.spyOn(host, 'fetch').mockResolvedValueOnce(new Response(null));
		expect(await procedure(z.object({}))).toEqual({});
	});

	it("names the failure from a host's XRPC error, or from its status when the body says nothing", async () => {
		const failing = (response: Response) => {
			vi.spyOn(host, 'fetch').mockResolvedValueOnce(response);

			return credentials.procedure({ space: SPACE, service: host.url, ...listRepos, body: {}, schema: z.object({}) });
		};

		await credentials.get(SPACE);

		await expect(
			failing(Response.json({ error: 'SpaceDeleted', message: 'gone' }, { status: 400 }))
		).rejects.toMatchObject({
			error: 'SpaceDeleted',
			message: 'SpaceDeleted: gone',
			nsid: listRepos.nsid
		});
		await expect(failing(new Response('Bad Gateway', { status: 502 }))).rejects.toMatchObject({
			error: 'UpstreamFailure',
			message: 'UpstreamFailure'
		});
		await expect(failing(new Response('Not Found', { status: 404 }))).rejects.toMatchObject({
			error: 'InvalidRequest'
		});
	});
});

describe('SpaceCredentialManager, given a space host from a DID document', () => {
	const HOST = 'pds.rebind.localnetwork.computer';
	let loopback: Loopback;

	beforeEach(async () => {
		loopback = await rebindToLoopback([HOST]);

		return async () => {
			vi.restoreAllMocks();
			await loopback.close();
		};
	});

	it('refuses a host that resolves to a private address when it connects, and never reaches it', async () => {
		const service = `https://${HOST}:${loopback.port}`;

		const identity = new IdentityResolver({
			plcUrl: 'https://plc.directory',
			allowPrivateNetwork: false,
			logger: silentLogger
		});

		vi.spyOn(identity, 'resolve').mockImplementation(async (did) => ({
			did,
			handle: null,
			pds: service,
			signingKey: '',
			kemKey: null
		}));

		const credentials = new SpaceCredentialManager({
			source: async () => ({ getDelegationToken: async () => 'delegation-token' }),
			identity,
			logger: silentLogger
		});

		await expect(credentials.request({ space: SPACE, service, ...listRepos })).rejects.toMatchObject({
			cause: { message: 'Hostname resolved to non-unicast address' }
		});
		expect(loopback.connections()).toBe(0);
	});
});
