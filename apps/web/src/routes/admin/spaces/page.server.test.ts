import { sql } from 'kysely';
import { afterEach, beforeEach, expect, it } from 'vitest';
import { createTestServices } from '#lib/server/testing/core.ts';
import { loadRoute } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { load } from './+page.server.ts';

const ADMIN = 'did:plc:admin';

const MOD = 'did:plc:mod';

const ADA = 'did:plc:ada';

let t: Awaited<ReturnType<typeof createTestServices>>;

beforeEach(async () => {
	t = await createTestServices({ admins: [ADMIN] });
	await t.staff.grant(await t.viewer(ADMIN), MOD, 'moderator');
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

it('streams every shared space to staff', async () => {
	const space = await t.sharedSpaces.create(await t.viewer(ADA), 'party');

	const data = await loadRoute<PageServerData>(load, { app: t, viewer: await t.viewer(MOD), url: '/admin/spaces' });

	expect(await data.spaces).toMatchObject([
		{ uri: space, kind: 'party', people: 1, listed: false, createdBy: { did: ADA, handle: null } }
	]);
});

it("shows the page without the list when the spaces can't be read", async () => {
	await sql`DROP TABLE shared_space`.execute(t.db);

	const data = await loadRoute<PageServerData>(load, { app: t, viewer: await t.viewer(MOD), url: '/admin/spaces' });

	expect(await data.spaces).toBeNull();
});

it('refuses an account without a role', async () => {
	await expect(
		loadRoute<PageServerData>(load, { app: t, viewer: await t.viewer(ADA), url: '/admin/spaces' })
	).rejects.toMatchObject({
		status: 403
	});
});
