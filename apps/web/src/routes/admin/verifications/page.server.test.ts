import { afterEach, beforeEach, expect, it } from 'vitest';
import { BSKY_PROFILE } from '#lib/nsid.ts';
import { HappyViewClient } from '#lib/server/happyview/client.ts';
import { IndexedRepo } from '#lib/server/happyview/indexed.ts';
import { silentLogger } from '#lib/server/logger.ts';
import { VerificationService } from '#lib/server/services/verifications.ts';
import { createTestServices } from '#lib/server/testing/core.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { actions, load } from './+page.server.ts';

const ADMIN = 'did:plc:admin';

const MOD = 'did:plc:mod';

const DJ = 'did:plc:dj';

let t: Awaited<ReturnType<typeof createTestServices>>;

beforeEach(async () => {
	t = await createTestServices({ admins: [ADMIN] });
	const admin = await t.viewer(ADMIN);
	await t.staff.grant(admin, MOD, 'moderator');

	await t.projectAdmin.create({
		actor: admin,
		profile: { name: 'Neon Tide', kind: 'solo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } },
		managers: [DJ]
	});

	t.handles.set(DJ, 'neon.test');
	await t.net
		.client(DJ)
		.putRecord({ collection: BSKY_PROFILE, rkey: 'self', record: { $type: BSKY_PROFILE, displayName: 'Neon Tide' } });
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

/** The app with an identity resolver that knows handles as `<name>.test`. */
const app = () => ({
	...t,
	identity: {
		resolveIdentifier: async (identifier: string) => {
			if (identifier.endsWith('.test')) return `did:plc:${identifier.slice(0, -'.test'.length)}`;

			throw new Error(`no such handle ${identifier}`);
		}
	}
});

it("lists the root's verifications, streaming each account's live handle and name", async () => {
	await t.verifications.grant(await t.viewer(ADMIN), DJ);

	const data = await loadRoute<PageServerData>(load, {
		app: app(),
		viewer: await t.viewer(ADMIN),
		url: `/admin/verifications?subject=${DJ}`
	});

	expect(data.verifications?.map((verification) => [verification.subject, verification.handle])).toEqual([
		[DJ, 'neon.test']
	]);
	expect(Object.keys(data.live)).toEqual([DJ]);
	expect(await data.live[DJ]).toEqual({ handle: 'neon.test', displayName: 'Neon Tide' });
	expect(data.subject).toBe(DJ);
});

it('shows the page without the list while HappyView is down', async () => {
	const down = new VerificationService(
		t.core,
		new IndexedRepo(
			new HappyViewClient({ url: 'http://happyview.test', fetch: async () => new Response('down', { status: 503 }) }),
			silentLogger,
			0
		),
		t.projects,
		{ resolve: async () => ({ handle: null }) }
	);

	const data = await loadRoute<PageServerData>(load, {
		app: { ...app(), verifications: down },
		viewer: await t.viewer(ADMIN),
		url: '/admin/verifications'
	});

	expect(data).toEqual({ verifications: null, live: {}, subject: '' });
});

it('verifies a manager by handle, then removes the verification', async () => {
	const viewer = await t.viewer(ADMIN);

	expect(
		await callRoute(actions.grant, {
			app: app(),
			viewer,
			url: '/admin/verifications?/grant',
			form: { subject: 'dj.test' }
		})
	).toEqual({ success: 'Verified @neon.test as “Neon Tide”' });
	expect(await t.verifications.statuses([DJ])).toEqual({ [DJ]: 'verified' });

	expect(
		await callRoute(actions.revoke, { app: app(), viewer, url: '/admin/verifications?/revoke', form: { did: DJ } })
	).toEqual({ success: 'Verification removed' });
	expect(await t.verifications.statuses([DJ])).toEqual({ [DJ]: 'unverified' });
});

it("says why an account can't be verified or unverified", async () => {
	const viewer = await t.viewer(ADMIN);

	expect(
		await callRoute(actions.grant, {
			app: app(),
			viewer,
			url: '/admin/verifications?/grant',
			form: { subject: 'mod.test' }
		})
	).toMatchObject({ status: 400, data: { error: 'Only project managers can be verified' } });
	expect(
		await callRoute(actions.revoke, { app: app(), viewer, url: '/admin/verifications?/revoke', form: { did: DJ } })
	).toMatchObject({ status: 400, data: { error: "That account isn't verified" } });
});

it('leaves verifications to admins', async () => {
	const viewer = await t.viewer(MOD);

	await expect(
		loadRoute<PageServerData>(load, { app: app(), viewer, url: '/admin/verifications' })
	).rejects.toMatchObject({
		status: 403
	});

	for (const action of [actions.grant, actions.revoke])
		await expect(
			callRoute(action, { app: app(), viewer, url: '/admin/verifications', form: { subject: 'dj.test', did: DJ } })
		).rejects.toMatchObject({ status: 403 });
	expect(await t.verifications.list()).toEqual([]);
});
