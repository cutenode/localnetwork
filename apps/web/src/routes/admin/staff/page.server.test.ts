import { XRPCError } from '@atproto/api';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { createTestServices } from '#lib/server/testing/core.ts';
import { FakeRootClient } from '#lib/server/testing/fakes.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { actions, load } from './+page.server.ts';

const ADMIN = 'did:plc:admin';

const MOD = 'did:plc:mod';

let t: Awaited<ReturnType<typeof createTestServices>>;

beforeEach(async () => {
	t = await createTestServices({ admins: [ADMIN] });
});

afterEach(async () => {
	vi.restoreAllMocks();
	await t.audit.idle();
	await t.db.destroy();
});

/** The app with an identity resolver that knows handles as `<name>.test`. */
const app = () => ({
	...t,
	identity: {
		resolveIdentifier: async (identifier: string) => {
			if (identifier.startsWith('did:')) return identifier;

			if (identifier.endsWith('.test')) return `did:plc:${identifier.slice(0, -'.test'.length)}`;

			throw new Error(`no such handle ${identifier}`);
		}
	}
});

it('lists the staff for an admin', async () => {
	const data = await loadRoute<PageServerData>(load, {
		app: app(),
		viewer: await t.viewer(ADMIN),
		url: '/admin/staff'
	});

	expect(data.staff.map((member) => member.did)).toContain(ADMIN);
});

it('sends someone signed out to sign in, and refuses a moderator', async () => {
	await expect(loadRoute<PageServerData>(load, { app: app(), url: '/admin/staff' })).rejects.toMatchObject({
		status: 303,
		location: '/login?next=%2Fadmin%2Fstaff'
	});

	const viewer = { ...(await t.viewer(MOD)), roles: ['moderator' as const], isStaff: true };

	await expect(loadRoute<PageServerData>(load, { app: app(), viewer, url: '/admin/staff' })).rejects.toMatchObject({
		status: 403
	});
});

it('sets what a moderator can manage', async () => {
	const viewer = await t.viewer(ADMIN);
	await t.staff.grant(viewer, MOD, 'moderator');

	const result = await callRoute(actions.grants, {
		app: app(),
		viewer,
		url: '/admin/staff',
		form: { did: MOD, grants: ['flagGroups', 'made-up'] }
	});

	expect(result).toEqual({ success: 'Grants saved' });
	expect(await t.staff.grants(MOD)).toEqual(['flagGroups']);
});

it('grants a role by handle, and removes it again', async () => {
	const viewer = await t.viewer(ADMIN);

	const granted = await callRoute(actions.grant, {
		app: app(),
		viewer,
		url: '/admin/staff?/grant',
		form: { role: 'moderator', subject: 'mod.test', note: 'Night shift' }
	});

	expect(granted).toEqual({ success: 'Role granted' });
	expect(await t.staff.roles(MOD)).toEqual(['moderator']);

	const revoked = await callRoute(actions.revoke, {
		app: app(),
		viewer,
		url: '/admin/staff?/revoke',
		form: { did: MOD }
	});

	expect(revoked).toEqual({ success: 'Role removed' });
	expect(await t.staff.roles(MOD)).toEqual([]);
});

it("shows who a removal that didn't reach the private spaces left there", async () => {
	const viewer = await t.viewer(ADMIN);
	await t.staff.grant(viewer, MOD, 'moderator');
	vi.spyOn(FakeRootClient.prototype, 'removeMember').mockRejectedValue(new XRPCError(502, 'UpstreamFailure'));

	await callRoute(actions.revoke, { app: app(), viewer, url: '/admin/staff?/revoke', form: { did: MOD } });

	const data = await loadRoute<PageServerData>(load, { app: app(), viewer, url: '/admin/staff' });

	expect(await data.strays).toEqual([{ did: MOD, handle: null, spaces: ['audit', 'moderation', 'staff', 'industry'] }]);
});

it('refuses a role that is neither admin nor moderator', async () => {
	const result = await callRoute(actions.grant, {
		app: app(),
		viewer: await t.viewer(ADMIN),
		url: '/admin/staff?/grant',
		form: { role: 'owner', subject: 'mod.test' }
	});

	expect(result).toMatchObject({ status: 400, data: { error: 'Pick a role' } });
	expect(await t.staff.roles(MOD)).toEqual([]);
});
