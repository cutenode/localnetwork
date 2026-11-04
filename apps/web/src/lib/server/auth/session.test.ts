import * as fc from 'fast-check';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { type Db, openDatabase } from '../db';
import { hashToken, TOKEN_PATTERN, WebSessions } from './session';

const ALICE = 'did:plc:alice';

const BOB = 'did:plc:bob';

describe('WebSessions', () => {
	let db: Db;
	let sessions: WebSessions;

	beforeEach(async () => {
		vi.useFakeTimers({ toFake: ['Date'] });
		db = await openDatabase(':memory:');
		sessions = new WebSessions(db, 30);
	});

	afterEach(async () => {
		vi.useRealTimers();
		await db.destroy();
	});

	const stored = () => db.selectFrom('webSession').selectAll().execute();

	it('hands out a token that resolves to its account, storing only its hash and a trimmed user agent', async () => {
		const token = await sessions.create(ALICE, 'x'.repeat(400));

		expect(token).toMatch(TOKEN_PATTERN);
		expect(await sessions.resolve(token)).toBe(ALICE);
		expect(await stored()).toEqual([
			expect.objectContaining({ tokenHash: hashToken(token), did: ALICE, userAgent: 'x'.repeat(300) })
		]);

		await sessions.create(ALICE, null);
		expect((await stored()).map((row) => row.userAgent)).toContain(null);
	});

	it('resolves no account for a missing, malformed, or unknown token', async () => {
		const token = await sessions.create(ALICE, null);

		expect(await sessions.resolve(undefined)).toBeNull();
		expect(await sessions.resolve('')).toBeNull();
		expect(await sessions.resolve(hashToken(token))).toBeNull();

		await fc.assert(
			fc.asyncProperty(fc.string(), async (guess) => {
				fc.pre(guess !== token);
				expect(await sessions.resolve(guess)).toBeNull();
			})
		);
	});

	it('expires a session after its days are up, and clears expired ones when the next is created', async () => {
		expect(sessions.maxAgeSeconds).toBe(30 * 86_400);
		const token = await sessions.create(ALICE, null);

		vi.setSystemTime(Date.now() + sessions.maxAgeSeconds * 1000 - 1);
		expect(await sessions.resolve(token)).toBe(ALICE);

		vi.setSystemTime(Date.now() + 1);
		expect(await sessions.resolve(token)).toBeNull();

		const next = await sessions.create(BOB, null);
		expect((await stored()).map((row) => row.tokenHash)).toEqual([hashToken(next)]);
	});

	it('signs out one session, or every session of an account, leaving the rest', async () => {
		const first = await sessions.create(ALICE, null);
		const second = await sessions.create(ALICE, null);
		const bobs = await sessions.create(BOB, null);

		await sessions.destroy(undefined);
		await sessions.destroy('not a token');
		await sessions.destroy(first);
		expect(await sessions.resolve(first)).toBeNull();
		expect(await sessions.resolve(second)).toBe(ALICE);

		await sessions.destroyAll(ALICE);
		expect(await sessions.resolve(second)).toBeNull();
		expect(await sessions.resolve(bobs)).toBe(BOB);
	});
});
