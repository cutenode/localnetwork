import { afterEach, beforeEach, expect, it } from 'vitest';
import type { VenueView } from '#lib/server/services/venues.ts';
import { CALENDAR_EVENT } from '#lib/nsid.ts';
import { createTestServices, ROOT } from '#lib/server/testing/core.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import type { Flags } from '#lib/types.ts';
import { actions, load } from './+page.server.ts';
import type { PageServerData } from './$types';

const ADMIN = 'did:plc:admin';

const OWNER = 'did:plc:owner';

const BOOKER = 'did:plc:booker';

const DJ = 'did:plc:dj';

let t: Awaited<ReturnType<typeof createTestServices>>;

let venue: VenueView;

/** An act's key, and a party's space and key. */
let actKey: string;

let partySpace: string;

let partyKey: string;

const keyOf = (space: string) => space.slice(space.lastIndexOf('/') + 1);

beforeEach(async () => {
	t = await createTestServices({ admins: [ADMIN] });
	const admin = await t.viewer(ADMIN);
	venue = await t.venues.add(admin, { name: 'Tresor', address: { country: 'DE', locality: 'Berlin' } });
	await t.sharedSpaces.addPerson(admin, venue.space, { did: OWNER, role: 'owner' });
	await t.sharedSpaces.addPerson(admin, venue.space, { did: BOOKER, role: 'booker' });
	actKey = keyOf(
		await t.sharedSpaces.createAct(await t.viewer(DJ), { slug: 'crew', form: 'duo', basics: { name: 'Crew' } })
	);
	partySpace = await t.parties.start(await t.viewer(DJ), {
		basics: { name: 'Night Shift' },
		avatar: 'keep',
		publish: true
	});
	partyKey = keyOf(partySpace);
	await t.accounts.recordLogin({
		did: DJ,
		handle: 'dj.test',
		pdsUrl: 'https://pds.test',
		grantedScope: 'atproto',
		spacesCapable: false,
		publicWrites: false
	});
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

const on: Partial<Flags> = { peopleSpaces: true };

/** The services, with handles resolving to the test accounts. */
const app = () => ({
	...t,
	identity: {
		resolveIdentifier: async (identifier: string) => {
			if (identifier === 'dj.test') return DJ;
			throw new Error(`no such handle ${identifier}`);
		}
	}
});

const opened = async (did: string, flags = on) =>
	loadRoute<PageServerData>(load, {
		app: app(),
		viewer: await t.viewer(did),
		url: `/dashboard/venues/${venue.skey}/settings`,
		params: { key: venue.skey },
		flags
	});

const post = async (action: keyof typeof actions, did: string, form: Record<string, string>) =>
	callRoute(actions[action], {
		app: app(),
		viewer: await t.viewer(did),
		url: `/dashboard/venues/${venue.skey}/settings?/${action}`,
		params: { key: venue.skey },
		flags: on,
		form
	});

it("shows a venue's owners its approval mode and blocks, by name", async () => {
	expect(await opened(OWNER)).toEqual({
		venue: { name: 'Tresor', approval: 'open' },
		can: { edit: true, block: true },
		blocks: []
	});

	for (const [kind, name] of [
		['profile', '@dj.test'],
		['act', `https://localnetwork.test/act/${actKey}/crew`],
		['party', `/party/${partyKey}`]
	])
		expect(await post('block', OWNER, { kind, name })).toEqual({ success: 'Blocked.' });

	expect((await opened(OWNER)).blocks.map(({ kind, name }) => [kind, name])).toEqual([
		['profile', '@dj.test'],
		['act', 'Crew'],
		['party', 'Night Shift']
	]);
});

it('keeps out everyone else: a 404 while peopleSpaces is off, a 403 without the capabilities', async () => {
	await expect(opened(OWNER, {})).rejects.toMatchObject({ status: 404 });
	await expect(opened(BOOKER)).rejects.toMatchObject({ status: 403 });
	await expect(opened(DJ)).rejects.toMatchObject({ status: 403 });
	await expect(post('block', BOOKER, { kind: 'act', name: actKey })).rejects.toMatchObject({ status: 403 });
	expect((await opened(ADMIN)).can).toEqual({ edit: true, block: true });

	await expect(
		loadRoute<PageServerData>(load, {
			app: app(),
			viewer: await t.viewer(OWNER),
			url: '/dashboard/venues/3nothere/settings',
			params: { key: '3nothere' },
			flags: on
		})
	).rejects.toMatchObject({ status: 404 });
});

it('shows someone who can only edit the page the approval mode, without the blocks', async () => {
	await post('block', OWNER, { kind: 'act', name: actKey });
	await t.sharedSpaces.updatePerson(await t.viewer(OWNER), venue.space, BOOKER, { capabilities: ['page.edit'] });

	expect(await opened(BOOKER)).toMatchObject({ can: { edit: true, block: false }, blocks: [] });
});

it('switches the approval mode', async () => {
	expect(await post('approval', OWNER, { approval: 'approval' })).toEqual({ success: 'Saved: Venue approves.' });
	expect((await opened(OWNER)).venue.approval).toBe('approval');

	expect(await post('approval', OWNER, { approval: 'open' })).toEqual({ success: 'Saved: Anyone can list.' });
	expect(await post('approval', OWNER, { approval: 'maybe' })).toMatchObject({
		status: 400,
		data: { error: 'Choose how events show' }
	});
});

it('says who it could not find, keeping what was typed, and takes a block away', async () => {
	expect(await post('block', OWNER, { kind: 'profile', name: 'nobody.test' })).toMatchObject({
		status: 400,
		data: { error: "There's no account called nobody.test", values: { kind: 'profile', name: 'nobody.test' } }
	});
	expect(await post('block', OWNER, { kind: 'party', name: actKey })).toMatchObject({
		data: { error: "There's no party at that link" }
	});
	expect(await post('block', OWNER, { kind: 'act', name: `/party/${partyKey}` })).toMatchObject({
		data: { error: "There's no act at that link" }
	});
	expect(await post('block', OWNER, { kind: 'robot', name: 'crew' })).toMatchObject({
		data: { error: 'Choose who to block' }
	});

	await post('block', OWNER, { kind: 'act', name: actKey });
	const [block] = await t.venues.blocks(venue.space);
	expect(await post('unblock', OWNER, { rkey: block.rkey })).toEqual({ success: 'Block taken away.' });
	expect((await opened(OWNER)).blocks).toEqual([]);
});

it('names a block whose act or party is gone', async () => {
	await post('block', OWNER, { kind: 'party', name: partySpace });
	await t.sharedSpaces.forget(partySpace);

	expect((await opened(OWNER)).blocks.map(({ name }) => name)).toEqual(['Someone no longer here']);
});

it("brings its events' calendar copies in line as its approval mode and blocks change", async () => {
	const admin = await t.viewer(ADMIN);
	venue = await t.venues.add(admin, {
		name: 'Halle',
		address: { country: 'DE', locality: 'Berlin' },
		geo: { latitude: '52.51', longitude: '13.42' }
	});
	await t.sharedSpaces.addPerson(admin, venue.space, { did: OWNER, role: 'owner' });

	const uri = await t.events.create(await t.viewer(DJ), partySpace, {
		name: 'Winter Opening',
		date: '2099-12-05',
		start: '23:00',
		venue: venue.space,
		audience: 'public',
		publish: true
	});

	const copied = () => t.net.records.has(`${ROOT}|${CALENDAR_EVENT}|${uri.split('/').at(-1)}`);
	expect(copied()).toBe(true);

	// Waiting for the venue isn't listed under Venue approves.
	await post('approval', OWNER, { approval: 'approval' });
	expect(copied()).toBe(false);
	await post('approval', OWNER, { approval: 'open' });
	expect(copied()).toBe(true);

	// Confirmed under Venue approves, until a block of its party means the venue never confirms it.
	await t.venues.setApproval(admin, venue.space, 'approval');
	await t.events.approve(admin, uri);
	await post('block', OWNER, { kind: 'party', name: partySpace });
	expect(copied()).toBe(false);
	const [block] = await t.venues.blocks(venue.space);
	await post('unblock', OWNER, { rkey: block.rkey });
	expect(copied()).toBe(true);
});
