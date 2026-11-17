import { SpaceMacError } from '@atproto/space';
import * as fc from 'fast-check';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { z } from 'zod';
import { silentLogger } from '../logger';
import { type Loopback, rebindToLoopback } from '../testing/rebind';
import { remoteObject } from '../testing/remote-bodies';
import { MEBIBYTE, streamed } from '../testing/streamed';
import { FakeSpaceHost } from '../testing/space-host';
import { SpaceCredentialManager, XrpcResponseError } from './credentials';
import { IdentityResolver } from './identity';
import { RecordValidationError } from './lexicons';
import { bookingSpace } from './spaces';

const SPACE = bookingSpace('did:plc:root', 'gig');

const DJ = 'did:plc:dj';

const NOTE = 'computer.localnetwork.test.note';

const listWriters = { nsid: 'com.atproto.space.listWriters', params: { space: SPACE }, audience: 'did:plc:root' };

const REGISTRATION = { endpoint: 'https://localnetwork.test', token: 'notify-token' };

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
		const response = await credentials.request({ space: SPACE, service: host.url, ...listWriters });
		expect(response.ok).toBe(true);

		const [ask, use] = host.requests;
		expect(ask.nsid).toBe('com.atproto.space.authorizeSpaceSync');
		expect(ask.body).toEqual({
			space: SPACE,
			credentialKey: expect.stringMatching(/^did:key:zDn/),
			delegationToken: 'delegation-token'
		});
		expect(ask.headers.get('authorization')).toBeNull();
		expect(ask.headers.get('signature')).toBeNull();
		expect(use.headers.get('authorization')).toMatch(/^Atproto-Space-Credential /);
		expect(use.headers.get('signature-input')).toContain('tag="atproto-space-credential"');
	});

	describe('with a host that sends more than any answer holds', () => {
		it.each([
			[
				'a query',
				'com.atproto.space.listWriters',
				'Response too large',
				() => credentials.query({ space: SPACE, service: host.url, ...listWriters, schema: z.unknown() })
			],
			[
				'a credential',
				'com.atproto.space.authorizeSpaceSync',
				'No credential returned',
				() => credentials.request({ space: SPACE, service: host.url, ...listWriters })
			]
		])('stops reading %s at the cap', async (_name, nsid, failure, call) => {
			const answer = streamed(64);

			host.overrides.set(nsid, () => new Response(answer.body));

			await expect(call()).rejects.toThrow(failure);
			expect(answer.made()).toBeLessThan(40 * MEBIBYTE);
		});

		it('still reads a repo export of any size', async () => {
			host.overrides.set('com.atproto.space.getRepo', () => new Response(streamed(33).body));

			const response = await credentials.request({
				space: SPACE,
				service: host.url,
				audience: DJ,
				nsid: 'com.atproto.space.getRepo',
				params: { space: SPACE, repo: DJ }
			});

			expect((await response.arrayBuffer()).byteLength).toBe(33 * MEBIBYTE);
		});
	});

	it('gets a fresh credential and tries once more when the host revoked the one it had', async () => {
		await credentials.request({ space: SPACE, service: host.url, ...listWriters });
		host.revoked.add(host.issued[0]);

		const response = await credentials.request({ space: SPACE, service: host.url, ...listWriters });
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
			'MAC does not match'
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

	it('rejects an answer it cannot read as an InvalidResponse from the method that sent it', async () => {
		const invalid = (nsid: string) => ({ status: 502, error: 'InvalidResponse', nsid });

		const listWritersCall = () =>
			credentials.query({ space: SPACE, service: host.url, ...listWriters, schema: z.unknown() });

		await host.put(DJ, NOTE, 'a', { $type: NOTE, text: 'hi' });

		// Deeper than lex JSON's recursion goes, and a key it refuses.
		for (const body of [`${'['.repeat(20_000)}${']'.repeat(20_000)}`, '{"__proto__":{}}', 'not json']) {
			host.overrides.set(listWriters.nsid, () => new Response(body));
			await expect(listWritersCall()).rejects.toMatchObject(invalid(listWriters.nsid));
		}

		host.overrides.set('com.atproto.space.getRecord', () => new Response('not json'));
		await expect(credentials.readRecord({ space: SPACE, repo: DJ, collection: NOTE, rkey: 'a' })).rejects.toMatchObject(
			invalid('com.atproto.space.getRecord')
		);

		host.overrides.set('com.atproto.space.getRecord', () => Response.json({ uri: 'at://x' }));
		await expect(credentials.readRecord({ space: SPACE, repo: DJ, collection: NOTE, rkey: 'a' })).rejects.toMatchObject(
			invalid('com.atproto.space.getRecord')
		);

		// A credential whose header part is base64url `null`, which the token parser reads a field of.
		credentials.invalidate(SPACE);
		host.overrides.set('com.atproto.space.authorizeSpaceSync', () => Response.json({ credential: 'bnVsbA.e30.c2ln' }));
		await expect(credentials.get(SPACE)).rejects.toMatchObject(invalid('com.atproto.space.authorizeSpaceSync'));
	});

	describe('with a notify registration for the space', () => {
		/** A manager whose root mints a new delegation token each time, as the host takes each one once. */
		const registering = () => {
			let minted = 0;

			return new SpaceCredentialManager({
				source: async () => ({ getDelegationToken: async () => `delegation-token-${++minted}` }),
				identity: host.identity,
				logger: silentLogger,
				notify: async (space) => (space === SPACE ? REGISTRATION : null)
			});
		};

		it("registers for the space's updates as it mints the credential, for as long as the credential lasts", async () => {
			const credential = await registering().get(SPACE);

			expect(host.requests[0].body).toMatchObject({ notify: REGISTRATION });
			expect(credential.notifying).toBe(true);
			expect(host.registrations).toEqual([{ ...REGISTRATION, expiresAt: credential.expiresAt }]);
		});

		it('still gets a credential, with a fresh delegation token, when the host cannot confirm the endpoint', async () => {
			host.confirmsRegistration = false;

			const credential = await registering().get(SPACE);

			expect(credential.notifying).toBe(false);
			expect(host.requests.map((request) => request.body)).toEqual([
				expect.objectContaining({ delegationToken: 'delegation-token-1', notify: REGISTRATION }),
				expect.not.objectContaining({ notify: expect.anything() })
			]);
			expect(host.requests[1].body).toMatchObject({ delegationToken: 'delegation-token-2' });
			expect(host.registrations).toEqual([]);
		});

		it('passes any other refusal on without trying again', async () => {
			host.credentialError = 'SpaceDeleted';

			await expect(registering().get(SPACE)).rejects.toMatchObject({
				error: 'SpaceDeleted',
				nsid: 'com.atproto.space.authorizeSpaceSync'
			});
			expect(host.requests).toHaveLength(1);
		});
	});

	it("names the failure from a host's XRPC error, or from its status when the body says nothing", async () => {
		const failing = (response: Response) => {
			vi.spyOn(host, 'fetch').mockResolvedValueOnce(response);

			return credentials.query({ space: SPACE, service: host.url, ...listWriters, schema: z.object({}) });
		};

		await credentials.get(SPACE);

		await expect(
			failing(Response.json({ error: 'SpaceDeleted', message: 'gone' }, { status: 400 }))
		).rejects.toMatchObject({
			error: 'SpaceDeleted',
			message: 'SpaceDeleted: gone',
			nsid: listWriters.nsid
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

describe('SpaceCredentialManager, given any answer from a space host', () => {
	let host: FakeSpaceHost;
	let credentials: SpaceCredentialManager;

	beforeAll(async () => {
		host = await FakeSpaceHost.create(SPACE);
		await host.put(DJ, NOTE, 'a', { $type: NOTE, text: 'hi' });
	});

	beforeEach(() => {
		host.overrides.clear();

		credentials = new SpaceCredentialManager({
			source: async () => ({ getDelegationToken: async () => 'delegation-token' }),
			identity: host.identity,
			logger: silentLogger
		});
	});

	/** Statuses whose answers have no body. */
	const BODILESS = new Set([204, 205, 304]);

	/** How a host can answer: any status, mostly 200, with any content type. */
	const answer = fc.record({
		status: fc.oneof(
			{ arbitrary: fc.constant(200), weight: 3 },
			{ arbitrary: fc.integer({ min: 200, max: 599 }), weight: 1 }
		),
		type: fc.constantFrom('application/json', 'text/html', 'application/octet-stream')
	});

	const respond = ({ status, type }: { status: number; type: string }, text: string) =>
		new Response(BODILESS.has(status) ? null : text, { status, headers: { 'content-type': type } });

	const b64 = (value: fc.JsonValue) => Buffer.from(JSON.stringify(value)).toString('base64url');

	/** Three base64url JSON parts, which is how far a credential's shape goes before its claims. */
	const tokenLike = fc
		.tuple(fc.jsonValue(), fc.jsonValue())
		.map(([header, payload]) => `${b64(header)}.${b64(payload)}.c2ln`);

	/** Whether `cause` is an error the client throws for an answer it won't take. */
	const isClientError = (cause: unknown) =>
		cause instanceof XrpcResponseError || cause instanceof RecordValidationError || cause instanceof SpaceMacError;

	/** What `call` settled with: its value, or the error it threw. */
	const settle = async <T>(call: Promise<T>): Promise<{ value: T } | { error: unknown }> => {
		try {
			return { value: await call };
		} catch (error) {
			return { error };
		}
	};

	/** How a settled call reads in a failed expectation. */
	const shown = (result: { value: unknown } | { error: unknown }) =>
		'error' in result ? String(result.error) : `resolved ${JSON.stringify(result.value)}`;

	const listWritersOutput = z.object({
		repos: z.array(
			z.object({ $type: z.string(), repo: z.string(), repoRev: z.string().optional(), spaceRev: z.string() })
		),
		cursor: z.string().optional()
	});

	it("resolves a query with output its schema takes, or rejects with the client's own errors", async () => {
		await fc.assert(
			fc.asyncProperty(
				answer,
				remoteObject({
					repos: fc.array(
						fc.record({ $type: fc.string(), repo: fc.string(), repoRev: fc.jsonValue(), spaceRev: fc.string() })
					)
				}),
				async (how, text) => {
					host.overrides.set(listWriters.nsid, () => respond(how, text));

					const result = await settle(
						credentials.query({ space: SPACE, service: host.url, ...listWriters, schema: listWritersOutput })
					);

					const kept =
						'error' in result ? isClientError(result.error) : listWritersOutput.safeParse(result.value).success;

					expect(kept, shown(result)).toBe(true);
				}
			),
			{ numRuns: 50 }
		);
	});

	it("names a host's failure from its status for any error body", async () => {
		await fc.assert(
			fc.asyncProperty(
				fc.integer({ min: 400, max: 599 }),
				remoteObject({ error: fc.jsonValue(), message: fc.jsonValue() }),
				async (status, text) => {
					host.overrides.set(listWriters.nsid, () => new Response(text, { status }));

					const call = credentials.request({ space: SPACE, service: host.url, ...listWriters });

					await expect(call).rejects.toBeInstanceOf(XrpcResponseError);
					await expect(call).rejects.toMatchObject({ status, error: expect.any(String) });
				}
			),
			{ numRuns: 50 }
		);
	});

	it('mints a credential, or rejects with an XRPC error, for any credential answer', async () => {
		await fc.assert(
			fc.asyncProperty(
				answer,
				remoteObject({ credential: fc.oneof(tokenLike, fc.string(), fc.jsonValue()) }),
				async (how, text) => {
					host.overrides.set('com.atproto.space.authorizeSpaceSync', () => respond(how, text));

					const result = await settle(credentials.get(SPACE));
					const kept = 'error' in result ? result.error instanceof XrpcResponseError : result.value.usable;

					expect(kept, shown(result)).toBe(true);
				}
			),
			{ numRuns: 50 }
		);
	});

	it("reads a record, or rejects with the client's own errors, for any getRecord answer", async () => {
		await fc.assert(
			fc.asyncProperty(
				answer,
				remoteObject({
					uri: fc.string(),
					cid: fc.string(),
					value: fc.oneof(fc.jsonValue(), fc.dictionary(fc.string(), fc.jsonValue()))
				}),
				async (how, text) => {
					host.overrides.set('com.atproto.space.getRecord', () => respond(how, text));

					const result = await settle(credentials.readRecord({ space: SPACE, repo: DJ, collection: NOTE, rkey: 'a' }));
					// Without a MAC from the host, no answer here is a record.
					const kept = 'error' in result ? isClientError(result.error) : result.value === null;

					expect(kept, shown(result)).toBe(true);
				}
			),
			{ numRuns: 50 }
		);
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

		await expect(credentials.request({ space: SPACE, service, ...listWriters })).rejects.toMatchObject({
			cause: { message: 'Hostname resolved to non-unicast address' }
		});
		expect(loopback.connections()).toBe(0);
	});
});
