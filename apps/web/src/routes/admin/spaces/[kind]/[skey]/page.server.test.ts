import { sql } from 'kysely';
import { afterEach, beforeEach, expect, it } from 'vitest';
import { parseSpaceUri } from '#lib/server/atproto/spaces.ts';
import { createTestServices, ROOT } from '#lib/server/testing/core.ts';
import { callRoute, loadRoute, type RouteEventInit } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { actions, load } from './+page.server.ts';

const ADMIN = 'did:plc:admin';

const MOD = 'did:plc:mod';

const ADA = 'did:plc:ada';

const BO = 'did:plc:bo';

const CY = 'did:plc:cy';

let t: Awaited<ReturnType<typeof createTestServices>>;

let space: string;

let skey: string;

beforeEach(async () => {
	t = await createTestServices({ admins: [ADMIN] });
	await t.staff.grant(await t.viewer(ADMIN), MOD, 'moderator');
	space = await t.sharedSpaces.create(await t.viewer(ADA), 'party');
	skey = parseSpaceUri(space)!.skey;
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

/** The app under the root, with an identity resolver that knows handles as `<name>.test`. */
const app = () => ({
	...t,
	rootDid: ROOT,
	identity: {
		resolveIdentifier: async (identifier: string) => {
			if (identifier.endsWith('.test')) return `did:plc:${identifier.slice(0, -'.test'.length)}`;

			throw new Error(`no such handle ${identifier}`);
		}
	}
});

/** The party's page as the moderator sees it, with `init` on top. */
const asMod = async (init: RouteEventInit = {}): Promise<RouteEventInit> => ({
	app: app(),
	viewer: await t.viewer(MOD),
	url: `/admin/spaces/party/${skey}`,
	params: { kind: 'party', skey },
	...init
});

/** Who is in the party, and in what role. */
const people = async () =>
	(await t.sharedSpaces.people(space, await t.viewer(MOD))).map((person) => [person.did, person.role, person.hidden]);

it("shows staff a space, who started it, its kind's roles, and its people, invites and requests to join", async () => {
	await t.spaceJoins.invite(await t.viewer(ADA), space, { did: BO, role: 'door' });
	await t.spaceJoins.requestToJoin(await t.viewer(CY), space, { message: 'Can I help?', hidden: false });

	const data = await loadRoute<PageServerData>(load, await asMod());

	expect(data.space).toMatchObject({ uri: space, kind: 'party', listed: false });
	expect(data.createdBy).toEqual({ did: ADA, handle: null });
	expect(data.roles).toEqual(['host', 'cohost', 'door']);
	expect((await data.people)?.map((person) => person.did)).toEqual([ADA]);
	expect(await data.pending).toMatchObject({
		invites: [{ did: BO, role: 'door' }],
		links: 0,
		requests: [{ did: CY, message: 'Can I help?' }]
	});
});

it("shows the space without its people, invites or requests when they can't be read", async () => {
	await sql`DROP TABLE space_person`.execute(t.db);
	await sql`DROP TABLE space_invite`.execute(t.db);

	const data = await loadRoute<PageServerData>(load, await asMod());

	expect(data.space.uri).toBe(space);
	expect(await data.people).toBeNull();
	expect(await data.pending).toBeNull();
});

it("answers 404 for a kind of space there isn't, or a space that isn't there", async () => {
	await expect(
		loadRoute<PageServerData>(load, await asMod({ params: { kind: 'project', skey } }))
	).rejects.toMatchObject({
		status: 404
	});
	await expect(loadRoute<PageServerData>(load, await asMod({ params: { kind: 'venue', skey } }))).rejects.toMatchObject(
		{
			status: 404
		}
	);
	await expect(
		callRoute(actions.list, await asMod({ params: { kind: 'project', skey }, form: {} }))
	).rejects.toMatchObject({ status: 404 });
});

it('adds someone by handle in a role, hidden if asked, then changes their role and removes them', async () => {
	expect(
		await callRoute(actions.add, await asMod({ form: { subject: 'bo.test', role: 'cohost', hidden: 'on' } }))
	).toEqual({ success: 'Added' });
	expect(await people()).toEqual([
		[ADA, 'host', false],
		[BO, 'cohost', true]
	]);

	expect(await callRoute(actions.role, await asMod({ form: { did: BO, role: 'door' } }))).toEqual({
		success: 'Role changed'
	});
	expect((await people())[1]).toEqual([BO, 'door', true]);

	expect(await callRoute(actions.remove, await asMod({ form: { did: BO } }))).toEqual({ success: 'Removed' });
	expect(await people()).toEqual([[ADA, 'host', false]]);
});

it("adds someone shown, and refuses a role the space's kind doesn't have", async () => {
	expect(await callRoute(actions.add, await asMod({ form: { subject: 'cy.test', role: 'cohost' } }))).toEqual({
		success: 'Added'
	});
	expect((await people())[1]).toEqual([CY, 'cohost', false]);

	expect(await callRoute(actions.add, await asMod({ form: { subject: 'bo.test', role: 'owner' } }))).toMatchObject({
		status: 400,
		data: { error: 'A party has no owner role' }
	});
});

it('lists the space in the directory and takes it out again', async () => {
	expect(await callRoute(actions.list, await asMod({ form: {} }))).toEqual({ success: 'Listed in the directory' });
	expect((await t.sharedSpaces.get(space))?.listed).toBe(true);

	expect(await callRoute(actions.unlist, await asMod({ form: {} }))).toEqual({
		success: 'Taken out of the directory'
	});
	expect((await t.sharedSpaces.get(space))?.listed).toBe(false);
});

it('refuses every action to an account without a role', async () => {
	const init = await asMod({ viewer: await t.viewer(ADA), form: { subject: 'bo.test', did: ADA, role: 'door' } });

	await expect(loadRoute<PageServerData>(load, init)).rejects.toMatchObject({ status: 403 });

	for (const action of [actions.add, actions.role, actions.remove, actions.list, actions.unlist])
		await expect(callRoute(action, init)).rejects.toMatchObject({ status: 403 });
	expect(await people()).toEqual([[ADA, 'host', false]]);
});
