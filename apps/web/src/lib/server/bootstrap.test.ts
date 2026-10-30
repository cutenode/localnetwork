import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { ensureRootSpaces } from './bootstrap';
import { createTestServices } from './testing/core';

describe('ensureRootSpaces', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;

	beforeEach(async () => {
		t = await createTestServices();
	});
	afterEach(async () => {
		await t.audit.idle();
		await t.db.destroy();
	});

	it('creates each root space once when two bootstraps overlap', async () => {
		const spaces = Object.values(t.spaces);

		for (const space of spaces) t.net.spaces.delete(space);

		await Promise.all([ensureRootSpaces(t.core), ensureRootSpaces(t.core)]);

		expect(spaces.filter((space) => !t.net.spaces.has(space))).toEqual([]);

		// One creation entry per space from the test setup, and one from whichever overlapping run created it.
		for (const space of spaces) expect(await t.audit.list({ action: 'system.space', subject: space })).toHaveLength(2);
	});

	it('closes an industry space that any signed-in account could read to staff only', async () => {
		const industry = t.net.spaces.get(t.spaces.industry)!;
		industry.readPolicy = { $type: 'com.atproto.simplespace.defs#publicPolicy' };

		await ensureRootSpaces(t.core);

		expect(industry.readPolicy.$type).toBe('com.atproto.simplespace.defs#memberListPolicy');
		expect(await t.audit.list({ action: 'system.space', subject: t.spaces.industry })).toMatchObject([
			{ summary: 'Corrected the industry space policies' },
			{ summary: 'Created the industry space' }
		]);
	});

	it("watches the root's performer, party and venue spaces again", async () => {
		const actor = await t.viewer('did:plc:ada');
		const spaces = [await t.sharedSpaces.create(actor, 'performer'), await t.sharedSpaces.create(actor, 'venue')];

		expect(await ensureRootSpaces(t.core)).toEqual(expect.arrayContaining(spaces));
	});
});
