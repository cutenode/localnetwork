import * as fc from 'fast-check';
import { beforeEach, expect, it } from 'vitest';
import { z } from 'zod';
import { parseJson } from '#lib/server/db/index.ts';
import type { Logger } from '#lib/server/logger.ts';
import { remoteObject } from '#lib/server/testing/remote-bodies.ts';
import { type WrittenLine, writtenLog } from '#lib/server/testing/written-log.ts';
import { callRoute } from '#lib/testing/route-event.ts';
import { POST } from './+server.ts';

const SPACE = 'at:space/did:plc:authority/computer.localnetwork.space.booking/self';

const TOKEN = 'registered-token';

/** A syncer that registered SPACE with TOKEN, and nothing else. */
const syncer = { isRegistered: async (space: string, token: string) => space === SPACE && token === TOKEN };

/** What the endpoint logged, through `log`. */
let logged: WrittenLine[];

let log: Logger;

beforeEach(() => {
	({ logger: log, lines: logged } = writtenLog('warn'));
});

/** Posts `body` to the endpoint, and returns its status and answer. */
async function confirm(body: string) {
	const response = await callRoute(POST, {
		app: { syncer },
		request: new Request('https://localnetwork.test/xrpc/com.atproto.space.confirmRegistration', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body
		}),
		log
	});

	return { status: response.status, body: await response.json() };
}

it('echoes the challenge for the token it registered the space with', async () => {
	const body = JSON.stringify({ space: SPACE, token: TOKEN, challenge: 'abc123' });

	expect(await confirm(body)).toEqual({ status: 200, body: { challenge: 'abc123' } });
});

it('refuses a token it did not register the space with, or a space it did not register', async () => {
	const refused = { status: 401, body: { error: 'AuthenticationRequired' } };
	const other = SPACE.replace('/self', '/other');

	expect(await confirm(JSON.stringify({ space: SPACE, token: 'guessed', challenge: 'abc123' }))).toEqual(refused);
	expect(await confirm(JSON.stringify({ space: other, token: TOKEN, challenge: 'abc123' }))).toEqual(refused);
	expect(logged.map((line) => line.message)).toEqual([
		'refused to confirm a notify registration',
		'refused to confirm a notify registration'
	]);
});

it('refuses a body that is not JSON, misses a field, or holds a token longer than the lexicon allows', async () => {
	const invalid = { status: 400, body: { error: 'InvalidRequest' } };

	expect(await confirm('not json')).toEqual(invalid);
	expect(await confirm(JSON.stringify({ space: SPACE, token: TOKEN }))).toEqual(invalid);
	expect(await confirm(JSON.stringify({ space: SPACE, token: 'x'.repeat(513), challenge: 'abc123' }))).toEqual(invalid);
});

it('echoes a challenge only for the token it registered the space with, and answers a 4xx to everything else', async () => {
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

	// Around the lexicon's 512 characters too.
	const challenge = fc.oneof(fc.string(), fc.string({ minLength: 500, maxLength: 520 }), fc.jsonValue());
	const fields = { space, token, challenge };

	const body = fc.oneof(
		{ arbitrary: fc.record(fields).map((value) => JSON.stringify(value)), weight: 3 },
		{ arbitrary: remoteObject(fields), weight: 1 }
	);

	/** The only bodies the endpoint should answer: the registered space and token, and a challenge the lexicon allows. */
	const registered = z.object({ space: z.literal(SPACE), token: z.literal(TOKEN), challenge: z.string().max(512) });

	await fc.assert(
		fc.asyncProperty(body, async (sent) => {
			const answer = await confirm(sent);
			const accepted = registered.safeParse(parseJson<unknown>(sent, null));

			expect(accepted.success ? [200] : [400, 401]).toContain(answer.status);
			expect(Object.keys(answer.body)).toEqual([accepted.success ? 'challenge' : 'error']);
			expect(answer.body.challenge).toBe(accepted.success ? accepted.data.challenge : undefined);
		}),
		{ numRuns: 100 }
	);
});
