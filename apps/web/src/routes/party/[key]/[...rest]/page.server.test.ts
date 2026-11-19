import { afterEach, beforeEach, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import { parseSpaceUri } from '#lib/server/atproto/spaces.ts';
import { createTestServices, ROOT } from '#lib/server/testing/core.ts';
import { callRoute, loadRoute, routeEvent } from '#lib/testing/route-event.ts';
import type { Flags } from '#lib/types.ts';
import { actions, load } from './+page.server.ts';
import type { PageServerData } from './$types';

const ADMIN = 'did:plc:admin';

const HOST = 'did:plc:host';

const COHOST = 'did:plc:cohost';

const DOOR = 'did:plc:door';

const STRANGER = 'did:plc:stranger';

let t: Awaited<ReturnType<typeof createTestServices>>;

let space: string;

let key: string;

const on: Partial<Flags> = { spacePages: true };

const app = () => ({ ...t, geocoder: { enabled: false }, config: { publicUrl: 'https://localnetwork.test' } });

const start = async (publish: boolean) => {
	space = await t.parties.start(await t.viewer(HOST), {
		basics: { name: 'Night Shift', bio: 'Techno on Thursdays.', location: { city: 'Berlin', country: 'DE' } },
		avatar: 'keep',
		publish
	});
	key = parseSpaceUri(space)!.skey;
	const admin = await t.viewer(ADMIN);
	await t.sharedSpaces.addPerson(admin, space, { did: COHOST, role: 'cohost', hidden: true });
	await t.sharedSpaces.addPerson(admin, space, { did: DOOR, role: 'door' });
};

const opened = async (did: string | null, path = `/party/${key}/night-shift`, flags = on) =>
	loadRoute<PageServerData>(load, {
		app: app(),
		viewer: did ? await t.viewer(did) : null,
		url: path,
		params: { key: path.split(/[/?]/)[2], rest: path.split('?')[0].split('/').slice(3).join('/') },
		flags
	});

const cid = () => t.net.spaceRecords.get(`${space}|${ROOT}|${COLLECTION.party}|self`)!.cid;

const save = async (did: string, form: Record<string, string>, path = `/party/${key}/night-shift?edit`) =>
	callRoute(actions.save, {
		app: app(),
		viewer: await t.viewer(did),
		url: `${path}&/save`,
		params: { key, rest: 'night-shift' },
		flags: on,
		form: { cid: cid(), ...form }
	});

beforeEach(async () => {
	t = await createTestServices({ admins: [ADMIN] });
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

it('shows a published party to anyone, with its hosts and cohosts who show', async () => {
	await start(true);

	const { headers, loadData } = routeEvent({
		app: app(),
		url: `/party/${key}/night-shift`,
		params: { key, rest: 'night-shift' },
		flags: on
	});

	const data = await loadData<PageServerData>(load);

	expect(data).toEqual({
		editing: false,
		version: cid(),
		status: 'unlisted',
		party: { basics: expect.objectContaining({ name: 'Night Shift' }), avatarUrl: null },
		events: [],
		newEventHref: null,
		people: [{ did: HOST, handle: null, role: 'host' }],
		geocoder: false,
		meta: expect.objectContaining({ title: 'Night Shift', description: 'Techno on Thursdays.', noindex: false })
	});
	expect(headers['cache-control']).toBe('public, max-age=60');

	await t.sharedSpaces.setListed(await t.viewer(ADMIN), space, true);
	expect((await opened(STRANGER)).status).toBe('listed');
});

it('gives its people with page.edit the edit bar, and opens the editor at ?edit', async () => {
	await start(true);

	expect(await opened(HOST, `/party/${key}/night-shift?edit`)).toMatchObject({
		subject: { kind: 'party', uri: space },
		can: expect.arrayContaining(['page.edit', 'events.manage']),
		role: 'owner',
		editing: true
	});
	// Door staff see the page as anyone does: it has nothing for them to change.
	expect(await opened(DOOR, `/party/${key}/night-shift?edit`)).toMatchObject({ role: 'owner', editing: false });
	expect(await opened(STRANGER, `/party/${key}/night-shift?edit`)).not.toHaveProperty('role');
});

it('sends any other name, or none, on to the current one, keeping the query', async () => {
	await start(true);

	await expect(opened(null, `/party/${key}`)).rejects.toMatchObject({
		status: 308,
		location: `/party/${key}/night-shift`
	});
	await expect(opened(HOST, `/party/${key}/old-name?edit`)).rejects.toMatchObject({
		status: 308,
		location: `/party/${key}/night-shift?edit`
	});
});

it('is not found while spacePages is off, for a key with no party, and for a draft to anyone outside it', async () => {
	await start(false);

	await expect(opened(HOST, `/party/${key}/night-shift`, {})).rejects.toMatchObject({ status: 404 });
	await expect(opened(null, '/party/3nothere')).rejects.toMatchObject({ status: 404 });
	await expect(opened(null)).rejects.toMatchObject({ status: 404 });
	await expect(opened(STRANGER)).rejects.toMatchObject({ status: 404 });

	expect(await opened(DOOR)).toMatchObject({ status: 'draft', meta: expect.objectContaining({ noindex: true }) });
	// Someone hidden stays off the page, even for the people in it.
	expect((await opened(COHOST)).people).toEqual([{ did: HOST, handle: null, role: 'host' }]);
	expect(await opened(ADMIN)).toMatchObject({ status: 'draft', role: 'staff' });
});

it('describes a party with no bio by its name, and previews its picture', async () => {
	space = await t.parties.start(await t.viewer(HOST), {
		basics: { name: 'Night Shift' },
		avatar: { upload: { bytes: new Uint8Array([1, 2, 3]), mimeType: 'image/png' } },
		publish: true
	});
	key = parseSpaceUri(space)!.skey;
	const { avatarUrl } = (await t.parties.bySpace(space))!;

	expect((await opened(null)).meta).toMatchObject({
		description: 'Night Shift, a party on Local Network.',
		image: { url: `https://localnetwork.test${avatarUrl}`, alt: 'Night Shift' }
	});
});

it('saves the page and goes back to its view, and publishes a draft', async () => {
	await start(false);

	await expect(
		save(COHOST, { name: 'Night Shift Late', bio: '', city: '', region: '', country: '', publish: 'on' })
	).rejects.toMatchObject({ status: 303, location: `/party/${key}/night-shift` });

	expect(await t.parties.bySpace(space)).toMatchObject({
		basics: { name: 'Night Shift Late' },
		publishedAt: expect.any(String)
	});
});

it('hands back the latest values when someone saved first, and the field a save was refused for', async () => {
	await start(true);
	const before = cid();

	await expect(
		save(HOST, { name: 'Night Shift', bio: 'Fridays now.', city: 'Berlin', country: 'DE' })
	).rejects.toMatchObject({ status: 303 });

	expect(await save(HOST, { name: 'Night Shift Late', cid: before })).toMatchObject({
		status: 409,
		data: {
			conflict: {
				version: cid(),
				values: { name: 'Night Shift', bio: 'Fridays now.', city: 'Berlin', region: '', country: 'DE' }
			}
		}
	});
	expect(await save(HOST, { name: '' })).toMatchObject({
		status: 400,
		data: { error: "Add the party's name", field: { name: 'name' } }
	});
	expect(await save(DOOR, { name: 'Taken over' })).toMatchObject({ status: 403 });
});
