import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { parseSpaceUri } from '#lib/server/atproto/spaces.ts';
import type { VenueView } from '#lib/server/services/venues.ts';
import { createTestServices } from '#lib/server/testing/core.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import type { Flags } from '#lib/types.ts';
import { actions, load } from './+page.server.ts';
import type { PageServerData } from './$types';

const ADMIN = 'did:plc:admin';

const HOST = 'did:plc:host';

const DOOR = 'did:plc:door';

let t: Awaited<ReturnType<typeof createTestServices>>;

let party: string;

let key: string;

let venue: VenueView;

const on: Partial<Flags> = { spacePages: true };

const app = () => ({ ...t, config: { publicUrl: 'https://localnetwork.test' } });

const opened = async (did: string | null, flags = on) =>
	loadRoute<PageServerData>(load, {
		app: app(),
		viewer: did ? await t.viewer(did) : null,
		url: `/party/${key}/new`,
		params: { key },
		flags
	});

const save = async (form: Record<string, string>) =>
	callRoute(actions.save, {
		app: app(),
		viewer: await t.viewer(HOST),
		url: `/party/${key}/new?/save`,
		params: { key },
		flags: on,
		form
	});

/** The one event made, by its key. */
const made = async () => (await t.events.ofParty(party))[0];

beforeEach(async () => {
	vi.useFakeTimers({ toFake: ['Date'] });
	vi.setSystemTime(new Date('2026-11-13T12:00:00Z'));
	t = await createTestServices({ admins: [ADMIN] });
	party = await t.parties.start(await t.viewer(HOST), {
		basics: { name: 'Night Shift', location: { city: 'Berlin', country: 'DE', latE6: 52_520_000, lngE6: 13_405_000 } },
		avatar: 'keep',
		publish: true
	});
	key = parseSpaceUri(party)!.skey;
	const admin = await t.viewer(ADMIN);
	await t.sharedSpaces.addPerson(admin, party, { did: DOOR, role: 'door' });
	venue = await t.venues.add(admin, {
		name: 'Tresor',
		address: { country: 'DE', locality: 'Berlin' },
		geo: { latitude: '52.51', longitude: '13.42' }
	});
});

afterEach(async () => {
	vi.useRealTimers();
	await t.audit.idle();
	await t.db.destroy();
});

it("opens an empty event page in edit mode for the party's people who run its events", async () => {
	expect(await opened(HOST)).toEqual({
		subject: { kind: 'event', uri: 'event:new' },
		can: ['page.edit', 'events.manage'],
		role: 'owner',
		party: { name: 'Night Shift', href: `/party/${key}/night-shift` },
		// A new draft starts on its party's area's time zone, until it has a venue.
		timeZone: 'Europe/Berlin',
		timeZones: expect.arrayContaining(['UTC', 'Europe/Berlin']),
		meta: expect.objectContaining({ title: 'New event', noindex: true })
	});
	await expect(opened(DOOR)).rejects.toMatchObject({ status: 403 });
	await expect(opened(HOST, {})).rejects.toMatchObject({ status: 404 });
	await expect(opened(null)).rejects.toMatchObject({
		status: 303,
		location: `/login?next=${encodeURIComponent(`/party/${key}/new`)}`
	});
});

it('saves a draft and goes on to edit it at its own address', async () => {
	const saving = save({
		name: 'Winter Opening',
		description: 'Doors at eleven.',
		venue: venue.space,
		audience: 'private'
	});

	await expect(saving).rejects.toMatchObject({ status: 303 });
	const { rkey, record } = await made();

	await expect(saving).rejects.toMatchObject({ location: `/party/${key}/${rkey}/winter-opening?edit` });
	expect(record).toMatchObject({
		name: 'Winter Opening',
		description: 'Doors at eleven.',
		venue: venue.space,
		audience: 'private'
	});
	expect(record.publishedAt).toBeUndefined();
});

it('makes and publishes it in one go, to its page', async () => {
	const saving = save({
		name: 'Winter Opening',
		venue: venue.space,
		date: '2026-12-05',
		start: '23:00',
		publish: 'on'
	});

	await expect(saving).rejects.toMatchObject({ status: 303 });
	const { rkey, record } = await made();

	await expect(saving).rejects.toMatchObject({ location: `/party/${key}/${rkey}/winter-opening` });
	expect(record).toMatchObject({ startsAt: '2026-12-05T23:00:00+01:00', publishedAt: '2026-11-13T12:00:00.000Z' });
});

it('needs a name, and says what else publishing needs, keeping what was typed', async () => {
	expect(await save({ name: '', description: 'Techno' })).toMatchObject({
		status: 400,
		data: { error: 'Give the event a name', field: { name: 'name' }, values: { name: '', description: 'Techno' } }
	});
	expect(await save({ name: 'Winter Opening', publish: 'on' })).toMatchObject({
		status: 400,
		data: { error: 'Add a date and a venue to publish it' }
	});
	expect(await t.events.ofParty(party)).toEqual([]);
});
