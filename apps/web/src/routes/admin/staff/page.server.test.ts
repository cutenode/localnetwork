import { afterEach, beforeEach, expect, it } from 'vitest';
import { createTestServices } from '#lib/server/testing/core.ts';
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
