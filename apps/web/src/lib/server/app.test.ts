import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { P256Keypair } from '@atproto/crypto';
import { cidForLex } from '@atproto/lex-cbor';
import { jsonToLex } from '@atproto/lex-json';
import { createSpaceToken } from '@atproto/space';
import pino from 'pino';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { COLLECTION, DIRECTORY, VERIFICATION } from '#lib/nsid.ts';
import { type App, createApp, getApp } from './app';
import { rootSpacePolicies } from './bootstrap';
import { loadConfig } from './config';
import { nowIso } from './db';
import { type Logger, silentLogger } from './logger';
import { FakeHappyView } from './testing/happyview';
import { fakeS3 } from './testing/s3';

const ROOT = 'did:plc:rootrootrootrootrootroot';

const NEON = 'did:plc:neonneonneonneonneonneon';

const SEARCH = `GET /xrpc/${DIRECTORY.searchProjects}`;

/** Config without a HappyView API key, so the app leaves HappyView's scripts alone. */
const NO_KEY = { HAPPYVIEW_API_KEY: undefined };

const ENV = {
	NODE_ENV: 'test',
	PUBLIC_URL: 'https://localnetwork.test',
	DATABASE_PATH: ':memory:',
	LOG_LEVEL: 'silent',
	ROOT_IDENTIFIER: ROOT,
	ROOT_PASSWORD: 'pw',
	ROOT_SERVICE: 'https://pds.test',
	PLC_URL: 'https://plc.test',
	HAPPYVIEW_URL: 'https://hv.test',
	HAPPYVIEW_API_KEY: 'hv_key',
	GEOCODER: 'none',
	ACTOR_SEARCH: 'none',
	SYNC_POLL_INTERVAL_MS: '1000',
	SYNC_NOTIFY: 'false'
};

type Route = (request: Request, url: URL) => Response | Promise<Response>;

let signingKey: string;

let authorityKey: P256Keypair;

beforeAll(async () => {
	authorityKey = await P256Keypair.create();
	signingKey = authorityKey.did().slice('did:key:'.length);
});

/**
 * Everything the app reaches over `fetch`, in process: a PLC directory putting every account on one PDS, that PDS
 * (the root's sign-in, space credentials, blobs), HappyView's admin API, and HappyView's queries from FakeHappyView.
 * `routes` are keyed `METHOD /path`, for a test to change one; anything without a route is a 404.
 */
function stubNetwork() {
	const seen: { method: string; host: string; path: string; headers: Headers; url: URL }[] = [];
	const happyView = new FakeHappyView(ROOT);

	const routes = new Map<string, Route>([
		[
			'POST /xrpc/com.atproto.server.createSession',
			() => Response.json({ did: ROOT, handle: 'root.test', accessJwt: 'a', refreshJwt: 'r' })
		],
		['GET /xrpc/com.atproto.space.getDelegationToken', () => Response.json({ token: 'delegation' })],
		[
			'POST /xrpc/com.atproto.space.getSpaceCredential',
			async (request) => {
				const { space, credentialKid } = await request.json();

				const credential = await createSpaceToken(
					'credential',
					{ iss: ROOT, sub: space, keyId: credentialKid },
					authorityKey
				);

				return Response.json({ credential });
			}
		],
		['GET /admin/scripts', () => Response.json([])],
		['POST /admin/scripts', () => Response.json({}, { status: 201 })],
		['POST /admin/lexicons', () => Response.json({})],
		['POST /admin/backfill', () => Response.json({ id: 'job', status: 'running' }, { status: 201 })],
		['GET /admin/backfill/status', () => Response.json([{ id: 'job', status: 'completed' }])]
	]);

	const fetch = async (input: string | URL | Request, init?: RequestInit) => {
		const request = new Request(input, init);
		const url = new URL(request.url);
		seen.push({ method: request.method, host: url.host, path: url.pathname, headers: request.headers, url });

		if (url.host === 'plc.test') {
			const did = decodeURIComponent(url.pathname.slice(1));

			return Response.json({
				'@context': ['https://www.w3.org/ns/did/v1', 'https://w3id.org/security/multikey/v1'],
				id: did,
				alsoKnownAs: [],
				verificationMethod: [
					{ id: `${did}#atproto`, type: 'Multikey', controller: did, publicKeyMultibase: signingKey }
				],
				service: [{ id: '#atproto_pds', type: 'AtprotoPersonalDataServer', serviceEndpoint: 'https://pds.test' }]
			});
		}

		const route = routes.get(`${request.method} ${url.pathname}`);

		if (route) return route(request, url);

		if (url.host === 'hv.test' && !url.pathname.startsWith('/admin/')) return happyView.fetch(request);

		return Response.json({ error: 'NotFound' }, { status: 404 });
	};

	vi.stubGlobal('fetch', fetch);

	return {
		routes,
		happyView,
		seen,
		/** How many backfills HappyView was asked for. */
		backfills: () => seen.filter((call) => call.host === 'hv.test' && call.path === '/admin/backfill').length
	};
}

/** A warn-level logger that keeps what it writes. */
function capture() {
	const lines: { msg: string; task?: string; collection?: string }[] = [];
	const logger: Logger = pino({ level: 'warn' }, { write: (line: string) => void lines.push(JSON.parse(line)) });

	return { logger, lines };
}

let net: ReturnType<typeof stubNetwork>;

const running: App[] = [];

const start = async (env: Record<string, string | undefined> = {}, logger: Logger = silentLogger) => {
	const app = await createApp({ config: loadConfig({ ...ENV, ...env }), logger });
	running.push(app);

	return app;
};

const stop = async (app: App) => {
	running.splice(running.indexOf(app), 1);
	await app.stop();
};

beforeEach(() => {
	net = stubNetwork();
});

afterEach(async () => {
	for (const app of running.splice(0)) await app.stop();
	vi.useRealTimers();
	vi.unstubAllGlobals();
	vi.unstubAllEnvs();
});

describe('createApp', () => {
	it('sends email through Resend and DMs through Bluesky chat when the config says so', async () => {
		const resend: URL[] = [];

		net.routes.set('POST /emails', (_request, url) => {
			resend.push(url);

			return Response.json({ id: 'email-1' });
		});
		net.routes.set('GET /xrpc/chat.bsky.convo.getConvoAvailability', () => Response.json({ canChat: true }));

		const app = await start({
			EMAIL_PROVIDER: 'resend',
			RESEND_API_KEY: 're_key',
			DM_PROVIDER: 'bluesky'
		});

		const signIn = await app.notificationChannels.onSignIn({
			did: NEON,
			scope: 'atproto account:email',
			optIn: true,
			session: async () => ({ email: 'neon@example.test', emailConfirmed: true })
		});

		expect(signIn).toBe('read');
		expect(resend.map((url) => url.href)).toEqual(['https://api.resend.com/emails']);

		expect(await app.dm?.availability(NEON)).toBe('ok');
		const chat = net.seen.find((call) => call.path === '/xrpc/chat.bsky.convo.getConvoAvailability');
		expect(chat?.headers.get('atproto-proxy')).toBe('did:web:api.bsky.chat#bsky_chat');
	});

	it('sends neither email nor DMs when both are off', async () => {
		const app = await start({ EMAIL_PROVIDER: 'none', DM_PROVIDER: 'none' });

		const signIn = await app.notificationChannels.onSignIn({
			did: NEON,
			scope: 'atproto account:email',
			optIn: true,
			session: async () => ({ email: 'neon@example.test', emailConfirmed: true })
		});

		// No confirmation email can go out, so the address waits without one.
		expect(signIn).toBe('limited');
		expect(app.dm).toBeNull();
	});

	it('gets user repo clients from OAuth sessions unless told otherwise', async () => {
		const app = await start();

		expect(await app.core.userClient(NEON)).toBeNull();
	});

	it("fetches a space's blob from the repo's PDS with a space credential", async () => {
		const blobs: URL[] = [];

		net.routes.set('GET /xrpc/com.atproto.space.getBlob', (request, url) => {
			blobs.push(url);
			const typed = url.searchParams.get('cid') === 'typed';

			return new Response(new Uint8Array([1, 2, 3]), {
				headers: typed ? { 'content-type': 'image/png' } : {}
			});
		});

		const app = await start();
		const space = app.spaces.industry;

		expect(await app.core.fetchSpaceBlob!(space, NEON, 'typed')).toEqual({
			bytes: new Uint8Array([1, 2, 3]),
			mimeType: 'image/png'
		});
		expect((await app.core.fetchSpaceBlob!(space, NEON, 'untyped')).mimeType).toBe('application/octet-stream');

		expect(Object.fromEntries(blobs[0].searchParams)).toEqual({ space, repo: NEON, cid: 'typed' });
		const call = net.seen.find((seen) => seen.path === '/xrpc/com.atproto.space.getBlob');
		expect(call?.headers.get('authorization')).toMatch(/^Atproto-Space-Credential /);
	});

	it("fetches a public blob from the account's PDS, refusing failures and blobs over the limit", async () => {
		const stream = (bytes: number) =>
			new ReadableStream({
				start(controller) {
					controller.enqueue(new Uint8Array(bytes));
					controller.close();
				}
			});

		// Answers fetchBlob's read from the PDS, which its own ast-grep-ignore in app.ts allows.
		// ast-grep-ignore: no-direct-pds-read
		net.routes.set('GET /xrpc/com.atproto.sync.getBlob', (_request, url) => {
			const cid = url.searchParams.get('cid');

			if (url.searchParams.get('did') !== NEON) return new Response(null, { status: 400 });

			if (cid === 'typed') return new Response(stream(3), { headers: { 'content-type': 'image/png' } });

			if (cid === 'untyped') return new Response(stream(3));

			if (cid === 'declared-large') return new Response(stream(3), { headers: { 'content-length': '100' } });

			if (cid === 'large') return new Response(stream(10));

			return new Response(null, { status: 404 });
		});

		const app = await start();

		expect(await app.core.fetchBlob!(NEON, 'typed', 5)).toEqual({ bytes: new Uint8Array(3), mimeType: 'image/png' });
		expect((await app.core.fetchBlob!(NEON, 'untyped', 5)).mimeType).toBe('application/octet-stream');
		await expect(app.core.fetchBlob!(NEON, 'gone', 5)).rejects.toThrow('getBlob gone: HTTP 404');
		await expect(app.core.fetchBlob!(NEON, 'declared-large', 5)).rejects.toThrow('blob declared-large is too large');
		await expect(app.core.fetchBlob!(NEON, 'large', 5)).rejects.toThrow('blob large is too large');
	});

	it('plays PDS recordings only for managers of an active project, as HappyView lists them', async () => {
		const app = await start();

		// HappyView lists no projects, so nobody manages one.
		expect(await app.recordings.audio(NEON, 'bafy')).toBeNull();
		expect(net.seen.some((call) => call.host === 'hv.test' && call.path.startsWith('/xrpc/'))).toBe(true);
	});

	it('registers for write notifications from space hosts when SYNC_NOTIFY is on', async () => {
		const registered: unknown[] = [];

		net.routes.set('POST /xrpc/com.atproto.space.registerNotify', async (request) => {
			registered.push(await request.json());

			return Response.json({});
		});

		const app = await start({ ...NO_KEY, SYNC_NOTIFY: 'true' });
		const space = app.spaces.staff;
		await app.syncer.watch(space);
		await app.syncer.sync(space).catch(() => undefined);

		expect(registered).toEqual([{ space, service: app.config.syncService }]);
	});

	it('reads the recordings of a manager of an active project', async () => {
		const project = {
			$type: COLLECTION.project,
			slug: 'neon',
			profile: {
				name: 'Neon',
				kind: 'solo',
				genres: ['techno'],
				location: { city: 'Berlin', country: 'DE', latE6: 52_520_000, lngE6: 13_405_000 }
			},
			managers: [NEON],
			status: 'active',
			revision: 1,
			createdBy: ROOT,
			createdAt: '2026-10-30T00:00:00.000Z',
			updatedAt: '2026-10-30T00:00:00.000Z'
		};

		const cid = (await cidForLex(jsonToLex(project))).toString();
		await net.happyView.ingest({ did: ROOT, collection: COLLECTION.project, rkey: '3neon', cid, record: project });
		const app = await start(NO_KEY);

		const reads = () => net.seen.filter((call) => call.url.searchParams.get('collection') === COLLECTION.djRecording);

		// Neon has no recordings, but as a manager their records are looked through.
		expect(await app.recordings.audio(NEON, 'bafy')).toBeNull();
		expect(reads().length).toBeGreaterThan(0);
	});

	it('watches a space a service asks for and syncs it in the background, keeping a failed sync for later', async () => {
		const app = await start(NO_KEY);
		const space = app.spaces.staff;

		// The PDS has no space reads, so the first sync fails without failing the call.
		await app.core.watchSpace!(space);
		await vi.waitFor(async () => {
			const watch = await app.db
				.selectFrom('spaceWatch')
				.select('lastError')
				.where('spaceUri', '=', space)
				.executeTakeFirstOrThrow();

			expect(watch.lastError).toBeTruthy();
		});
	});

	it('reports no backups without a bucket', async () => {
		const app = await start();

		expect(app.backupStatus).toBeNull();
	});
});

describe('syncDirectory', () => {
	it("uploads the app's scripts and has HappyView re-read the older records once per script", async () => {
		const app = await start({ USERINPUT_SPACE: `at://${NEON}/app.userinput.space/bugs` });

		expect(app.directorySync).toEqual({ at: expect.any(String), error: null });
		const uploads = net.seen.filter((call) => call.method === 'POST' && call.path === '/admin/scripts');
		expect(uploads.length).toBeGreaterThan(0);

		const refreshed = await app.db
			.selectFrom('kv')
			.select('key')
			.where('key', 'like', 'happyview_refreshed:%')
			.execute();

		expect(refreshed.map((row) => row.key)).toEqual(
			expect.arrayContaining([
				`happyview_refreshed:${COLLECTION.djSet}`,
				`happyview_refreshed:${VERIFICATION}`,
				'happyview_refreshed:app.userinput.space'
			])
		);

		// Nothing changed since, so a second round asks for nothing.
		const before = net.backfills();
		await app.refreshRecords();
		expect(net.backfills()).toBe(before);
	});

	it("keeps a failed sync's error for the System page instead of throwing", async () => {
		net.routes.set('GET /admin/scripts', () => new Response('down', { status: 500 }));
		const app = await start();

		expect(app.directorySync).toEqual({ at: expect.any(String), error: expect.stringContaining('HTTP 500') });
	});

	it("keeps an error that isn't an Error as text", async () => {
		// A body that fails with a bare string, which is what reading it rejects with.
		net.routes.set(
			'GET /admin/scripts',
			() =>
				new Response(
					new ReadableStream({
						start(controller) {
							controller.error('connection reset');
						}
					})
				)
		);
		const app = await start();

		expect(app.directorySync?.error).toBe('connection reset');
	});

	it("logs the collections HappyView couldn't re-read, and still marks the sync done", async () => {
		// The project search fails, so the managers to re-read can't be listed.
		net.routes.set(SEARCH, () => new Response('down', { status: 500 }));
		const { logger, lines } = capture();
		const app = await start({}, logger);

		expect(app.directorySync?.error).toBeNull();

		const failed = lines.filter((line) => line.msg === 'could not have HappyView re-read older records');
		expect(failed.map((line) => line.collection)).toContain(COLLECTION.djSet);
		expect(lines.some((line) => line.msg === 'could not re-read shows')).toBe(true);

		const verification = await app.db
			.selectFrom('kv')
			.select('key')
			.where('key', '=', `happyview_refreshed:${VERIFICATION}`)
			.executeTakeFirst();

		expect(verification).toBeDefined();
	});

	it('leaves HappyView alone without an API key', async () => {
		const app = await start(NO_KEY);

		await app.refreshRecords();
		expect(app.directorySync).toBeNull();
		expect(net.seen.some((call) => call.host === 'hv.test')).toBe(false);
	});
});

describe('start', () => {
	beforeEach(() => {
		vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'Date'] });
	});

	const getSpaces = () => net.seen.filter((call) => call.path === '/xrpc/com.atproto.space.getSpace').length;

	it('runs the sync on its interval, logs a round that fails, and skips rounds while one still runs', async () => {
		let release = () => {};

		const { logger, lines } = capture();
		const app = await start({}, logger);
		app.start();

		// Bootstrap's first call to the PDS fails, so the round is logged.
		await vi.advanceTimersByTimeAsync(1_000);
		expect(getSpaces()).toBe(1);
		await vi.waitFor(() =>
			expect(lines).toContainEqual(expect.objectContaining({ msg: 'background task failed', task: 'sync' }))
		);

		// Now it hangs: the next rounds wait for it.
		net.routes.set(
			'GET /xrpc/com.atproto.space.getSpace',
			() =>
				new Promise<Response>((resolve) => {
					release = () => resolve(Response.json({ error: 'NotFound' }, { status: 404 }));
				})
		);
		await vi.advanceTimersByTimeAsync(1_000);
		await vi.advanceTimersByTimeAsync(1_000);
		await vi.advanceTimersByTimeAsync(1_000);
		expect(getSpaces()).toBe(2);

		release();
		await vi.advanceTimersByTimeAsync(1_000);
		await vi.waitFor(() => expect(getSpaces()).toBe(3));
	});

	it('retries a failed directory sync, then re-reads older records on later rounds', async () => {
		net.routes.set('GET /admin/scripts', () => new Response('down', { status: 500 }));
		net.routes.set(SEARCH, () => new Response('down', { status: 500 }));
		const app = await start();
		app.start();
		expect(app.directorySync?.error).toContain('HTTP 500');

		net.routes.set('GET /admin/scripts', () => Response.json([]));
		await vi.advanceTimersByTimeAsync(1_000);
		await vi.waitFor(() => expect(app.directorySync?.error).toBeNull());

		// The manager collections still wait on the project search, so each round asks for them again.
		const lists = () => net.seen.filter((call) => call.path === `/xrpc/${DIRECTORY.searchProjects}`).length;
		const before = lists();
		await vi.advanceTimersByTimeAsync(1_000);
		await vi.waitFor(() => expect(lists()).toBeGreaterThan(before));
	});

	it('does nothing to HappyView on its rounds without an API key', async () => {
		const app = await start(NO_KEY);
		app.start();

		await vi.advanceTimersByTimeAsync(3_000);
		expect(net.seen.some((call) => call.host === 'hv.test')).toBe(false);
	});

	it('syncs the spaces on later rounds once a bootstrap has gone through', async () => {
		const policies = new Map<string, unknown>();

		net.routes.set('GET /xrpc/com.atproto.space.getSpace', (_request, url) => {
			const uri = url.searchParams.get('space')!;

			return Response.json({ uri, policy: policies.get(uri) });
		});
		net.routes.set('GET /xrpc/com.atproto.space.listSpaces', () => Response.json({ spaces: [] }));
		net.routes.set('GET /xrpc/com.atproto.memberspace.listMembers', () => Response.json({ members: [] }));
		// Answers the root's reads of its own repo at bootstrap.
		// ast-grep-ignore: no-direct-pds-read
		net.routes.set('GET /xrpc/com.atproto.repo.listRecords', () => Response.json({ records: [] }));

		const app = await start(NO_KEY);

		for (const { uri, policy } of rootSpacePolicies(app.spaces)) policies.set(uri, policy);
		app.start();

		const lists = () => net.seen.filter((call) => call.path === '/xrpc/com.atproto.space.listRepos').length;

		// The first round bootstraps, which syncs every root space once.
		await vi.advanceTimersByTimeAsync(1_000);
		await vi.waitFor(() => expect(lists()).toBe(rootSpacePolicies(app.spaces).length));

		// Later rounds sync again without checking the spaces exist.
		await vi.advanceTimersByTimeAsync(1_000);
		await vi.waitFor(() => expect(lists()).toBe(2 * rootSpacePolicies(app.spaces).length));
		expect(getSpaces()).toBe(rootSpacePolicies(app.spaces).length);
	});

	it("logs that it couldn't resume notification delivery when the outbox can't be read", async () => {
		const { logger, lines } = capture();
		const app = await start(NO_KEY, logger);
		await app.db.schema.dropTable('notificationDelivery').execute();

		app.start();
		await vi.waitFor(() =>
			expect(lines).toContainEqual(expect.objectContaining({ msg: 'could not resume notification delivery' }))
		);
	});

	it('clears OAuth states older than an hour each hour, and nothing once stopped', async () => {
		// A sync round every ten days, so the hour doesn't bring a sync each second.
		const app = await start({ ...NO_KEY, SYNC_POLL_INTERVAL_MS: '864000000' });
		const old = new Date(Date.now() - 2 * 60 * 60_000).toISOString();

		await app.db
			.insertInto('oauthState')
			.values([
				{ key: 'old', value: '{}', updatedAt: old },
				{ key: 'new', value: '{}', updatedAt: nowIso() }
			])
			.execute();
		app.start();

		await vi.advanceTimersByTimeAsync(60 * 60_000);
		await vi.waitFor(async () => {
			const keys = await app.db.selectFrom('oauthState').select('key').execute();
			expect(keys.map((row) => row.key)).toEqual(['new']);
		});

		await stop(app);
		const calls = net.seen.length;
		await vi.advanceTimersByTimeAsync(60 * 60_000);
		expect(net.seen.length).toBe(calls);
		await expect(app.db.selectFrom('kv').selectAll().execute()).rejects.toThrow('driver has already been destroyed');
	});
});

describe('backups', () => {
	it('backs the database up when started, then on the first hourly round of each new day', async () => {
		vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'Date'] });
		const s3 = await fakeS3();
		const dir = await mkdtemp(join(tmpdir(), 'app-backup-'));

		try {
			const databasePath = join(dir, 'app.db');

			// A sync round every ten days, so a day's hourly rounds don't bring a sync each second.
			const app = await start({
				...NO_KEY,
				DATABASE_PATH: databasePath,
				SYNC_POLL_INTERVAL_MS: '864000000',
				...s3.env
			});

			expect(app.backupStatus).toEqual({ latestAt: null, lastAttempt: null, stale: false });
			app.start();

			await s3.stored(1);

			vi.setSystemTime(Date.now() + 86_400_000);
			await vi.advanceTimersByTimeAsync(60 * 60_000);
			await s3.stored(2);
			await stop(app);
		} finally {
			await s3.close();
			await rm(dir, { recursive: true, force: true });
		}
	});
});

describe('getApp', () => {
	it('starts one app for the process, and tries again after a failed start', async () => {
		for (const [name, value] of Object.entries({ ...ENV, ...NO_KEY })) vi.stubEnv(name, value);
		net.routes.set('POST /xrpc/com.atproto.server.createSession', () =>
			Response.json({ error: 'AuthenticationRequired' }, { status: 401 })
		);

		await expect(getApp()).rejects.toThrow('AuthenticationRequired');

		net.routes.set('POST /xrpc/com.atproto.server.createSession', () =>
			Response.json({ did: ROOT, handle: 'root.test', accessJwt: 'a', refreshJwt: 'r' })
		);
		const app = await getApp();
		running.push(app);

		expect(app.rootDid).toBe(ROOT);
		expect(await getApp()).toBe(app);
	});
});

describe('viewer', () => {
	it('names an account the app has never seen without roles', async () => {
		const app = await start();

		expect(await app.viewer(NEON)).toEqual({
			did: NEON,
			handle: null,
			roles: [],
			isAdmin: false,
			isStaff: false,
			spacesCapable: false
		});
	});
});
