import { afterEach, beforeEach, expect, it } from 'vitest';
import { createTestServices } from '#lib/server/testing/core.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import type { PageServerData as ListData } from './$types';
import { load as loadList } from './+page.server.ts';
import { actions as flagActions, load as loadFlag } from './[key]/+page.server.ts';
import { actions as groupsActions, load as loadGroups } from './groups/+page.server.ts';
import { actions as groupActions, load as loadGroup } from './groups/[rkey]/+page.server.ts';

const ADMIN = 'did:plc:admin';

const MOD = 'did:plc:mod';

const FLAG = 'parityCanary';

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

			const { IdentityError } = await import('#lib/server/atproto/identity.ts');

			throw new IdentityError(`Could not resolve @${identifier}`);
		}
	}
});

const moderator = async (...grants: ('flags' | 'flagGroups')[]) => {
	const admin = await t.viewer(ADMIN);
	await t.staff.grant(admin, MOD, 'moderator');
	await t.staff.setGrants(admin, MOD, grants);

	return t.viewer(MOD);
};

it('lists every flag for an admin', async () => {
	const data = await loadRoute<ListData>(loadList, { app: app(), viewer: await t.viewer(ADMIN), url: '/admin/flags' });

	expect(data.flags.map((flag) => flag.key)).toContain(FLAG);
});

it('sends someone signed out to sign in, and refuses staff without the grant', async () => {
	await expect(loadRoute(loadList, { app: app(), url: '/admin/flags' })).rejects.toMatchObject({
		status: 303,
		location: '/login?next=%2Fadmin%2Fflags'
	});

	const viewer = await moderator('flagGroups');

	await expect(loadRoute(loadList, { app: app(), viewer, url: '/admin/flags' })).rejects.toMatchObject({ status: 403 });
	await expect(
		loadRoute(loadFlag, { app: app(), viewer, url: `/admin/flags/${FLAG}`, params: { key: FLAG } })
	).rejects.toMatchObject({ status: 403 });
	await expect(
		callRoute(flagActions.save, { app: app(), viewer, params: { key: FLAG }, url: `/admin/flags/${FLAG}`, form: {} })
	).rejects.toMatchObject({ status: 403 });
});

it('keeps a moderator with only the flags grant out of the groups pages', async () => {
	const viewer = await moderator('flags');

	await expect(loadRoute(loadGroups, { app: app(), viewer, url: '/admin/flags/groups' })).rejects.toMatchObject({
		status: 403
	});
	await expect(
		callRoute(groupsActions.create, { app: app(), viewer, url: '/admin/flags/groups', form: { name: 'Beta' } })
	).rejects.toMatchObject({ status: 403 });
	await expect(
		callRoute(groupActions.delete, { app: app(), viewer, url: '/admin/flags/groups/x', params: { rkey: 'x' } })
	).rejects.toMatchObject({ status: 403 });
});

it('saves who a flag is on for, resolving handles to DIDs', async () => {
	const viewer = await moderator('flags');
	const url = `/admin/flags/${FLAG}`;

	const result = await callRoute(flagActions.save, {
		app: app(),
		viewer,
		url,
		params: { key: FLAG },
		form: { everyone: 'on', percentage: '30', accounts: '@milo.test\ndid:plc:ada\nmilo.test' }
	});

	expect(result).toEqual({ success: 'Flag saved' });
	expect(await t.flags.flag(FLAG)).toMatchObject({
		everyone: true,
		percentage: 30,
		accounts: [{ did: 'did:plc:milo' }, { did: 'did:plc:ada' }]
	});
});

it('says which account it could not find, and saves nothing', async () => {
	const viewer = await t.viewer(ADMIN);

	const result = await callRoute(flagActions.save, {
		app: app(),
		viewer,
		url: `/admin/flags/${FLAG}`,
		params: { key: FLAG },
		form: { percentage: '0', accounts: 'nobody.example' }
	});

	expect(result).toMatchObject({ status: 400, data: { error: 'Could not resolve @nobody.example' } });
	expect((await t.flags.flag(FLAG))?.updatedAt).toBeNull();
});

it('reads an empty percentage as none', async () => {
	const result = await callRoute(flagActions.save, {
		app: app(),
		viewer: await t.viewer(ADMIN),
		url: `/admin/flags/${FLAG}`,
		params: { key: FLAG },
		form: { percentage: '', accounts: 'milo.test' }
	});

	expect(result).toEqual({ success: 'Flag saved' });
	expect(await t.flags.flag(FLAG)).toMatchObject({ percentage: 0 });
});

it('refuses a percentage out of range', async () => {
	const result = await callRoute(flagActions.save, {
		app: app(),
		viewer: await t.viewer(ADMIN),
		url: `/admin/flags/${FLAG}`,
		params: { key: FLAG },
		form: { percentage: '140' }
	});

	expect(result).toMatchObject({ status: 400, data: { error: 'Percentage must be a whole number from 0 to 100' } });
});

it('opens a flag with the groups it could use, and a 404 for one that does not exist', async () => {
	const viewer = await t.viewer(ADMIN);
	await t.flags.createGroup(viewer, { name: 'Beta', members: [] });

	const data = await loadRoute<{ flag: { key: string }; groups: { name: string }[] }>(loadFlag, {
		app: app(),
		viewer,
		url: `/admin/flags/${FLAG}`,
		params: { key: FLAG }
	});

	expect(data.flag.key).toBe(FLAG);
	expect(data.groups.map((group) => group.name)).toEqual(['Beta']);
	await expect(
		loadRoute(loadFlag, { app: app(), viewer, url: '/admin/flags/nope', params: { key: 'nope' } })
	).rejects.toMatchObject({ status: 404 });
});

it('creates a group and goes to it, then changes and deletes it', async () => {
	const viewer = await moderator('flagGroups');

	await expect(
		callRoute(groupsActions.create, {
			app: app(),
			viewer,
			url: '/admin/flags/groups',
			form: { name: 'Beta', description: 'Early', members: 'milo.test' }
		})
	).rejects.toMatchObject({ status: 303, location: expect.stringMatching(/^\/admin\/flags\/groups\/\w+$/) });
	const [{ rkey }] = await t.flags.groups();
	const params = { rkey };
	const url = `/admin/flags/groups/${rkey}`;

	expect(await loadRoute(loadGroup, { app: app(), viewer, url, params })).toMatchObject({
		group: { name: 'Beta', members: [{ did: 'did:plc:milo' }] }
	});

	expect(
		await callRoute(groupActions.save, { app: app(), viewer, url, params, form: { name: 'Early', members: '' } })
	).toEqual({ success: 'Group saved' });
	expect(await t.flags.group(rkey)).toMatchObject({ name: 'Early', description: null, members: [] });

	await expect(callRoute(groupActions.delete, { app: app(), viewer, url, params })).rejects.toMatchObject({
		status: 303,
		location: '/admin/flags/groups'
	});
	await expect(loadRoute(loadGroup, { app: app(), viewer, url, params })).rejects.toMatchObject({ status: 404 });
});

it('lists the groups for whoever manages them', async () => {
	const viewer = await moderator('flagGroups');
	await t.flags.createGroup(await t.viewer(ADMIN), { name: 'Beta', members: [] });

	expect(await loadRoute(loadGroups, { app: app(), viewer, url: '/admin/flags/groups' })).toMatchObject({
		groups: [{ name: 'Beta' }]
	});
});

it('lets a failure to resolve an account through, unless the account is what is wrong', async () => {
	const broken = { ...app(), identity: { resolveIdentifier: () => Promise.reject(new Error('the resolver is down')) } };

	await expect(
		callRoute(flagActions.save, {
			app: broken,
			viewer: await t.viewer(ADMIN),
			url: `/admin/flags/${FLAG}`,
			params: { key: FLAG },
			form: { percentage: '0', accounts: 'milo.test' }
		})
	).rejects.toThrow('the resolver is down');
});

it('asks a group for a name', async () => {
	const result = await callRoute(groupsActions.create, {
		app: app(),
		viewer: await t.viewer(ADMIN),
		url: '/admin/flags/groups',
		form: { name: ' ' }
	});

	expect(result).toMatchObject({ status: 400, data: { error: 'Give the group a name' } });
});

it('shows the reason a group in use stays', async () => {
	const viewer = await t.viewer(ADMIN);
	const rkey = await t.flags.createGroup(viewer, { name: 'Beta', members: [] });
	await t.flags.updateFlag(viewer, FLAG, { everyone: false, percentage: 0, groups: [rkey], dids: [] });

	const result = await callRoute(groupActions.delete, {
		app: app(),
		viewer,
		url: `/admin/flags/groups/${rkey}`,
		params: { rkey }
	});

	expect(result).toMatchObject({ status: 400, data: { error: `Take it off ${FLAG} first` } });
});
