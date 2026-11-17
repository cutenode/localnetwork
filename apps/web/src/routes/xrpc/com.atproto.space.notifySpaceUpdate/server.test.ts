import * as fc from 'fast-check';
import { beforeEach, expect, it } from 'vitest';
import { z } from 'zod';
import { parseJson } from '#lib/server/db/index.ts';
import type { Logger } from '#lib/server/logger.ts';
import { remoteObject } from '#lib/server/testing/remote-bodies.ts';
import { type WrittenLine, writtenLog } from '#lib/server/testing/written-log.ts';
import { callRoute } from '#lib/testing/route-event.ts';
import { POST } from './+server.ts';

const SPACE = 'at://did:plc:authority/space/computer.localnetwork.space.booking/self';

const TOKEN = 'registered-token';

/** A syncer that registered SPACE with TOKEN, keeps which spaces it synced and how, and fails while `down`. */
class FakeSyncer {
	readonly runs: [string, string][] = [];
	down = false;

	async isRegistered(space: string, token: string) {
		return space === SPACE && token === TOKEN;
	}

	async sync(space: string) {
		this.run('sync', space);
	}

	async reauthorize(space: string) {
		this.run('reauthorize', space);
	}

	private run(how: string, space: string) {
		if (this.down) throw new Error('space host unreachable');
		this.runs.push([how, space]);
	}
}

let syncer: FakeSyncer;

/** What the endpoint logged, through `log`. */
let logged: WrittenLine[];

let log: Logger;

beforeEach(() => {
	syncer = new FakeSyncer();
	({ logger: log, lines: logged } = writtenLog('warn'));
});

/** Posts `body` to the endpoint, and returns its status and answer once the sync it started ran. */
async function notify(body: string) {
	const response = await callRoute(POST, {
		app: { syncer },
		request: new Request('https://localnetwork.test/xrpc/com.atproto.space.notifySpaceUpdate', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body
		}),
		log
	});

	await new Promise((resolve) => setImmediate(resolve));

	return { status: response.status, body: await response.json() };
}

const update = ($type: string, token = TOKEN) =>
	JSON.stringify({ space: SPACE, token, update: { $type, repo: 'did:plc:writer', spaceRev: '3mwcj3b6uak2f' } });

it.each(['com.atproto.space.defs#repoWrite', 'com.atproto.space.defs#repoRemoved', 'app.example.newKind'])(
	'syncs the space on a %s update',
	async ($type) => {
		expect(await notify(update($type))).toEqual({ status: 200, body: {} });
		expect(syncer.runs).toEqual([['sync', SPACE]]);
	}
);

it.each([
	'com.atproto.space.notifySpaceUpdate#spaceDeleted',
	'com.atproto.space.notifySpaceUpdate#registrationExpired'
])('syncs with a fresh credential on a %s notice, so the authority confirms it', async ($type) => {
	expect(await notify(update($type))).toEqual({ status: 200, body: {} });
	expect(syncer.runs).toEqual([['reauthorize', SPACE]]);
});

it('answers at once and logs the failure when the sync it started fails', async () => {
	syncer.down = true;

	expect(await notify(update('com.atproto.space.defs#repoWrite'))).toEqual({ status: 200, body: {} });
	expect(logged.map((line) => line.message)).toEqual(['notified sync failed']);
});

it('refuses an update without the token it registered the space with', async () => {
	expect(await notify(update('com.atproto.space.notifySpaceUpdate#spaceDeleted', 'guessed'))).toEqual({
		status: 401,
		body: { error: 'AuthenticationRequired' }
	});
	expect(syncer.runs).toEqual([]);
	expect(logged.map((line) => line.message)).toEqual(['rejected notifySpaceUpdate']);
});

it('refuses a body that is not JSON, or names no update', async () => {
	const invalid = { status: 400, body: { error: 'InvalidRequest' } };

	expect(await notify('not json')).toEqual(invalid);
	expect(await notify(JSON.stringify({ space: SPACE, token: TOKEN }))).toEqual(invalid);
	expect(syncer.runs).toEqual([]);
});

it('syncs only on an update carrying the token it registered the space with, and answers a 4xx to everything else', async () => {
	// Mostly the registered space and token, so an accepted update comes up in most runs.
	const space = fc.oneof(
		{ arbitrary: fc.constant(SPACE), weight: 3 },
		{ arbitrary: fc.constant(SPACE.replace('/self', '/other')), weight: 1 },
		{ arbitrary: fc.jsonValue(), weight: 1 }
	);

	const token = fc.oneof(
		{ arbitrary: fc.constant(TOKEN), weight: 3 },
		{ arbitrary: fc.constantFrom('guessed', TOKEN.slice(1), `${TOKEN}x`), weight: 1 },
		{ arbitrary: fc.jsonValue(), weight: 1 }
	);

	const updates = fc.oneof(
		fc.record({
			$type: fc.oneof(
				fc.constantFrom('com.atproto.space.defs#repoWrite', 'com.atproto.space.notifySpaceUpdate#spaceDeleted'),
				fc.jsonValue()
			)
		}),
		fc.jsonValue()
	);

	const fields = { space, token, update: updates };

	const body = fc.oneof(
		{ arbitrary: fc.record(fields).map((value) => JSON.stringify(value)), weight: 3 },
		{ arbitrary: remoteObject(fields), weight: 1 }
	);

	/** The only bodies the endpoint should act on: the registered space and token, and an update with a type. */
	const registered = z.object({
		space: z.literal(SPACE),
		token: z.literal(TOKEN),
		update: z.object({ $type: z.string() })
	});

	await fc.assert(
		fc.asyncProperty(body, async (sent) => {
			const before = syncer.runs.length;
			const { status } = await notify(sent);
			const accepted = registered.safeParse(parseJson<unknown>(sent, null)).success;

			expect(accepted ? [200] : [400, 401]).toContain(status);
			expect(syncer.runs.length - before).toBe(accepted ? 1 : 0);
		}),
		{ numRuns: 100 }
	);
});
