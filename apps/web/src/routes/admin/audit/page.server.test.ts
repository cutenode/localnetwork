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
	await t.staff.grant(await t.viewer(ADMIN), MOD, 'moderator', 'Night shift');
	await t.audit.idle();
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

/** The audit page as the moderator sees it at `url`. */
const audit = async (url: string) => loadRoute<PageServerData>(load, { app: app(), viewer: await t.viewer(MOD), url });

it('lists the whole log newest first, with its stats and the action groups to filter by', async () => {
	const data = await audit('/admin/audit');

	// Bootstrapping the root's spaces logged one entry per space and one for the whole run.
	expect(data.entries.map((entry) => entry.action)).toEqual([
		'role.grant',
		'system.bootstrap',
		'system.space',
		'system.space',
		'system.space',
		'system.space'
	]);
	expect(data.entries[0].actor).toEqual({ did: ADMIN, handle: null });
	expect(data.stats.total).toBe(6);
	expect(data.orphans).toBe(0);
	expect(data.actionGroups).toContain('role');
	expect(data.actionGroups).not.toContain('role.grant');
	expect(data.filters).toEqual({ actor: '', action: '', outcome: '', q: '' });
});

it("filters by an actor's handle, action, outcome, and text, and echoes the filters back", async () => {
	const data = await audit('/admin/audit?actor=%20admin.test%20&action=role&outcome=success&q=mod');

	expect(data.entries.map((entry) => [entry.action, entry.subject])).toEqual([['role.grant', MOD]]);
	expect(data.filters).toEqual({ actor: 'admin.test', action: 'role', outcome: 'success', q: 'mod' });
});

it("filters by the actor as typed when it isn't a handle it can resolve", async () => {
	expect((await audit(`/admin/audit?actor=${ADMIN}`)).entries).toHaveLength(1);
	expect((await audit('/admin/audit?actor=nobody.example')).entries).toEqual([]);
});

it('pages back to entries before a sequence number', async () => {
	const [newest] = (await audit('/admin/audit')).entries;
	const older = await audit(`/admin/audit?before=${newest.seq}`);

	expect(older.entries.map((entry) => entry.seq)).toEqual([5, 4, 3, 2, 1]);
	expect((await audit('/admin/audit?before=soon')).entries).toHaveLength(6);
});

it('verifies the hash chain on request', async () => {
	const result = await callRoute(actions.verify, {
		app: app(),
		viewer: await t.viewer(MOD),
		url: '/admin/audit?/verify',
		form: {}
	});

	expect(result).toEqual({ verification: { ok: true, checked: 6 } });
});

it('refuses an account without a role', async () => {
	const viewer = { ...(await t.viewer('did:plc:dj')), isStaff: false };

	await expect(loadRoute<PageServerData>(load, { app: app(), viewer, url: '/admin/audit' })).rejects.toMatchObject({
		status: 403
	});
});
