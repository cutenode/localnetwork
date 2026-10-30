import { createHash } from 'node:crypto';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { type Db, openDatabase } from '../db';
import { silentLogger } from '../logger';
import { IdentityResolver } from './identity';
import { RootAccount } from './root';

const DID = 'did:plc:root';

describe('RootAccount', () => {
	let db: Db;
	let signIns: number;

	beforeEach(async () => {
		db = await openDatabase(':memory:');
		signIns = 0;
		// A PDS that hands out a fresh session for a sign-in and keeps one alive on refresh.
		vi.stubGlobal('fetch', async (input: URL) => {
			if (input.pathname.endsWith('createSession')) signIns++;
			const session = { did: DID, handle: 'root.test', accessJwt: `a${signIns}`, refreshJwt: `r${signIns}` };

			return Response.json(session);
		});
	});

	afterEach(async () => {
		vi.unstubAllGlobals();
		await db.destroy();
	});

	const connect = (password: string) =>
		new RootAccount({
			identifier: DID,
			password,
			service: 'https://pds.test',
			db,
			identity: new IdentityResolver({ plcUrl: 'https://plc.test', allowPrivateNetwork: false, logger: silentLogger }),
			logger: silentLogger
		}).connect();

	it("hands out an agent that sends chat calls to Bluesky chat, once it's connected", async () => {
		const proxied: (string | null)[] = [];

		// The PDS as above, which also answers chat calls, noting where each was to be proxied.
		vi.stubGlobal('fetch', async (input: Request | URL, init?: RequestInit) => {
			const request = new Request(input, init);

			if (request.url.endsWith('createSession')) {
				return Response.json({ did: DID, handle: 'root.test', accessJwt: 'a', refreshJwt: 'r' });
			}

			proxied.push(request.headers.get('atproto-proxy'));

			return Response.json({ canChat: true });
		});

		const root = new RootAccount({
			identifier: DID,
			password: 'pw',
			service: 'https://pds.test',
			db,
			identity: new IdentityResolver({ plcUrl: 'https://plc.test', allowPrivateNetwork: false, logger: silentLogger }),
			logger: silentLogger
		});

		expect(() => root.chat('did:web:api.bsky.chat')).toThrow('not connected');
		await root.connect();

		await root.chat('did:web:api.bsky.chat').chat.bsky.convo.getConvoAvailability({ members: ['did:plc:neon'] });
		expect(proxied).toEqual(['did:web:api.bsky.chat#bsky_chat']);
	});

	it('resumes the stored session, and signs in again once the password changes', async () => {
		await connect('first');
		await connect('first');
		expect(signIns).toBe(1);

		await connect('second');
		expect(signIns).toBe(2);
	});

	it('stores nothing the password can be cheaply checked against', async () => {
		await connect('hunter2');

		const { value } = await db
			.selectFrom('kv')
			.select('value')
			.where('key', '=', 'root_session')
			.executeTakeFirstOrThrow();

		const sha256 = (text: string) => createHash('sha256').update(text).digest('hex');

		expect(value).not.toContain('hunter2');
		expect(value).not.toContain(sha256(`${DID}:hunter2`));
		expect(value).not.toContain(sha256('hunter2'));
	});
});
