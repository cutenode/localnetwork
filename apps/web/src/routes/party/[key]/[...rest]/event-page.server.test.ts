import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import { parseSpaceRecordUri, parseSpaceUri } from '#lib/server/atproto/spaces.ts';
import { performerUri } from '#lib/server/performer-uri.ts';
import type { EventInput } from '#lib/server/services/events.ts';
import type { VenueView } from '#lib/server/services/venues.ts';
import { createTestServices, ROOT } from '#lib/server/testing/core.ts';
import { EVENT_STATUS } from '#lib/shows.ts';
import { callRoute, loadRoute, routeEvent } from '#lib/testing/route-event.ts';
import type { Flags } from '#lib/types.ts';
import { actions, load } from './+page.server.ts';
import type { PageServerData } from './$types';

const ADMIN = 'did:plc:admin';

const HOST = 'did:plc:host';

const DOOR = 'did:plc:door';

const DJ = 'did:plc:dj';

const STRANGER = 'did:plc:stranger';

let t: Awaited<ReturnType<typeof createTestServices>>;

let party: string;

let key: string;

let venue: VenueView;

const on: Partial<Flags> = { spacePages: true };

/** The services, with the DJ's handle resolving to them. */
const app = () => ({
	...t,
	geocoder: { enabled: false },
	config: { publicUrl: 'https://localnetwork.test' },
	identity: {
		resolveIdentifier: async (identifier: string) => {
			if (identifier === 'dj.test') return DJ;
			throw new Error(`no such handle ${identifier}`);
		}
	}
});

const input = (overrides: Partial<EventInput> = {}): EventInput => ({
	name: 'Winter Opening',
	date: '2026-12-05',
	start: '23:00',
	end: '06:00',
	venue: venue.space,
	audience: 'public',
	publish: true,
	...overrides
});

/** Makes an event in the party as its host, and gives back its key. */
const make = async (overrides: Partial<EventInput> = {}) =>
	parseSpaceRecordUri(await t.events.create(await t.viewer(HOST), party, input(overrides)))!.rkey;

const uriOf = (rkey: string) => `${party}/${ROOT}/${COLLECTION.event}/${rkey}`;

const stored = (rkey: string) => t.net.spaceRecords.get(`${party}|${ROOT}|${COLLECTION.event}|${rkey}`)!;

const paramsOf = (path: string) => ({
	key: path.split(/[/?]/)[2],
	rest: path.split('?')[0].split('/').slice(3).join('/')
});

const opened = async (did: string | null, path: string) =>
	loadRoute<PageServerData>(load, {
		app: app(),
		viewer: did ? await t.viewer(did) : null,
		url: path,
		params: paramsOf(path),
		flags: on
	});

const post = async (action: keyof typeof actions, did: string, path: string, form: Record<string, string | string[]>) =>
	callRoute(actions[action], {
		app: app(),
		viewer: await t.viewer(did),
		url: `${path}${path.includes('?') ? '&' : '?'}/${action}`,
		params: paramsOf(path),
		flags: on,
		form
	});

/** What the event page's form sends for the event as made, with `changes`. */
const form = (rkey: string, changes: Record<string, string | string[]> = {}) => ({
	cid: stored(rkey).cid,
	name: 'Winter Opening',
	description: '',
	venue: venue.space,
	date: '2026-12-05',
	start: '23:00',
	end: '06:00',
	tickets: '',
	audience: 'public',
	performer: '',
	setStart: '',
	setEnd: '',
	...changes
});

beforeEach(async () => {
	vi.useFakeTimers({ toFake: ['Date'] });
	vi.setSystemTime(new Date('2026-11-13T12:00:00Z'));
	t = await createTestServices({ admins: [ADMIN] });
	const admin = await t.viewer(ADMIN);
	party = await t.parties.start(await t.viewer(HOST), {
		basics: { name: 'Night Shift' },
		avatar: 'keep',
		publish: true
	});
	key = parseSpaceUri(party)!.skey;
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

it('shows a published event to anyone at its own address under the party, waiting for the venue', async () => {
	const rkey = await make({ tickets: 'https://tickets.example/winter', description: 'Doors at eleven.' });
	const path = `/party/${key}/${rkey}/winter-opening`;

	const { headers, loadData } = routeEvent({ app: app(), url: path, params: paramsOf(path), flags: on });

	expect(await loadData<PageServerData>(load)).toEqual({
		editing: false,
		timeZones: [],
		version: stored(rkey).cid,
		status: 'listed',
		event: {
			name: 'Winter Opening',
			description: 'Doors at eleven.',
			startsAt: '2026-12-05T23:00:00+01:00',
			endsAt: '2026-12-06T06:00:00+01:00',
			tickets: 'https://tickets.example/winter',
			status: EVENT_STATUS.scheduled,
			audience: 'public',
			venue: { space: venue.space, skey: venue.skey, name: 'Tresor', locality: 'Berlin', country: 'DE' },
			timeZone: 'Europe/Berlin',
			confirmed: false,
			neverConfirmed: false,
			lineup: []
		},
		party: { name: 'Night Shift', href: `/party/${key}/night-shift` },
		link: null,
		meta: expect.objectContaining({ title: 'Winter Opening', description: 'Doors at eleven.', noindex: false })
	});
	expect(headers['cache-control']).toBe('public, max-age=60');
});

it("sends any other name, or none, on to the event's current one, keeping the link's token", async () => {
	const rkey = await make();

	await expect(opened(null, `/party/${key}/${rkey}`)).rejects.toMatchObject({
		status: 308,
		location: `/party/${key}/${rkey}/winter-opening`
	});
	await expect(opened(HOST, `/party/${key}/${rkey}/old-name?t=abc&edit`)).rejects.toMatchObject({
		status: 308,
		location: `/party/${key}/${rkey}/winter-opening?t=abc&edit`
	});
	// Anything that isn't one of the party's events is the party's own name.
	await expect(opened(null, `/party/${key}/3lzzzzzzzzzzz/winter-opening`)).rejects.toMatchObject({
		status: 308,
		location: `/party/${key}/night-shift`
	});
});

it('opens a private event only from its link, uncached, and hands its people the link', async () => {
	const rkey = await make({ audience: 'private' });
	const { linkToken } = (await t.events.get(uriOf(rkey)))!.record;
	const path = `/party/${key}/${rkey}/winter-opening`;

	await expect(opened(STRANGER, path)).rejects.toMatchObject({ status: 404 });
	await expect(opened(null, `${path}?t=wrong`)).rejects.toMatchObject({ status: 404 });

	const { headers, loadData } = routeEvent({
		app: app(),
		url: `${path}?t=${linkToken}`,
		params: paramsOf(path),
		flags: on
	});

	expect(await loadData<PageServerData>(load)).toMatchObject({
		status: 'private',
		link: null,
		meta: expect.objectContaining({ noindex: true, url: `https://localnetwork.test${path}` })
	});
	expect(headers['cache-control']).toBe('no-store');
	expect(await opened(HOST, path)).toMatchObject({ link: `https://localnetwork.test${path}?t=${linkToken}` });
	// Door staff open it as its people do, but don't run its events, so they aren't handed the link to pass on.
	expect(await opened(DOOR, path)).toMatchObject({ status: 'private', link: null });
});

it("keeps a draft to the party's people and staff, and opens its editor for those who run its events", async () => {
	const rkey = await make({ publish: false, date: undefined, start: undefined, end: undefined });
	const path = `/party/${key}/${rkey}/winter-opening`;

	await expect(opened(STRANGER, path)).rejects.toMatchObject({ status: 404 });
	await expect(opened(null, path)).rejects.toMatchObject({ status: 404 });

	expect(await opened(HOST, `${path}?edit`)).toMatchObject({
		subject: { kind: 'event', uri: uriOf(rkey) },
		can: expect.arrayContaining(['page.edit', 'events.manage']),
		role: 'owner',
		editing: true,
		status: 'draft',
		event: { startsAt: undefined, venue: expect.objectContaining({ name: 'Tresor' }) },
		meta: expect.objectContaining({ noindex: true })
	});
	// Door staff can't change it, so they see it as it is.
	const door = await opened(DOOR, `${path}?edit`);
	expect(door).toMatchObject({ role: 'owner', editing: false, status: 'draft' });
	expect(door.can).not.toContain('page.edit');
	expect(await opened(ADMIN, `${path}?edit`)).toMatchObject({ role: 'staff', editing: true });
});

it('names its lineup by handle and act, with set times on the venue clock', async () => {
	const act = await t.sharedSpaces.createAct(await t.viewer(HOST), {
		slug: 'crew',
		form: 'duo',
		basics: { name: 'Crew' }
	});

	const actKey = parseSpaceUri(act)!.skey;
	await t.accounts.recordLogin({
		did: DJ,
		handle: 'dj.test',
		pdsUrl: null,
		grantedScope: '',
		spacesCapable: false,
		publicWrites: false
	});

	const rkey = await make({
		lineup: [
			{ performer: performerUri(ROOT, { kind: 'act', skey: actKey }), start: '23:00', end: '02:00' },
			{ performer: performerUri(ROOT, { kind: 'dj', did: DJ }) },
			{ performer: performerUri(ROOT, { kind: 'dj', did: 'did:plc:quiet' }) }
		]
	});

	// An act that has since left the site, as another app could have written it.
	const { record } = (await t.events.get(uriOf(rkey)))!;
	const gone = { performer: performerUri(ROOT, { kind: 'act', skey: '3gone' }) };
	await t.sharedSpaces.index({
		space: party,
		collection: COLLECTION.event,
		rkey,
		value: { ...record, lineup: [...(record.lineup ?? []), gone] }
	});

	expect((await opened(null, `/party/${key}/${rkey}/winter-opening`)).event?.lineup).toEqual([
		{ name: 'Crew', href: null, value: `/act/${actKey}/crew`, start: '23:00', end: '02:00' },
		{ name: '@dj.test', href: '/@dj.test', value: 'dj.test', start: undefined, end: undefined },
		{ name: 'did:plc:quiet', href: '/@did:plc:quiet', value: 'did:plc:quiet', start: undefined, end: undefined },
		{ name: 'Someone no longer here', href: null, value: '', start: undefined, end: undefined }
	]);
});

it('saves the event over the version it opened on, publishes a draft, and goes back to its view', async () => {
	const rkey = await make({ publish: false });
	const path = `/party/${key}/${rkey}/winter-opening?edit`;

	const act = await t.sharedSpaces.createAct(await t.viewer(HOST), {
		slug: 'crew',
		form: 'duo',
		basics: { name: 'Crew' }
	});

	const actKey = parseSpaceUri(act)!.skey;

	await expect(
		post('save', HOST, path, {
			...form(rkey, { name: 'Winter Closing', audience: 'private', tickets: 'https://tickets.example/w' }),
			performer: ['@dj.test', '', `https://localnetwork.test/act/${actKey}/crew`],
			setStart: ['02:00', '', '23:00'],
			setEnd: ['', '', '02:00'],
			publish: 'on'
		})
	).rejects.toMatchObject({ status: 303, location: `/party/${key}/${rkey}/winter-opening` });

	expect(stored(rkey).value).toMatchObject({
		name: 'Winter Closing',
		audience: 'private',
		tickets: 'https://tickets.example/w',
		publishedAt: '2026-11-13T12:00:00.000Z',
		lineup: [
			{ performer: performerUri(ROOT, { kind: 'dj', did: DJ }), startsAt: '2026-12-06T02:00:00+01:00' },
			{
				performer: performerUri(ROOT, { kind: 'act', skey: actKey }),
				startsAt: '2026-12-05T23:00:00+01:00',
				endsAt: '2026-12-06T02:00:00+01:00'
			}
		]
	});
});

it('hands back the latest values when someone saved first, and the field a save was refused for', async () => {
	const rkey = await make({ description: 'Doors at eleven.' });
	const path = `/party/${key}/${rkey}/winter-opening?edit`;
	const before = stored(rkey).cid;

	await expect(post('save', HOST, path, form(rkey, { description: 'Doors at ten.' }))).rejects.toMatchObject({
		status: 303
	});

	expect(await post('save', HOST, path, form(rkey, { name: 'Winter Closing', cid: before }))).toMatchObject({
		status: 409,
		data: {
			conflict: {
				version: stored(rkey).cid,
				values: {
					name: 'Winter Opening',
					description: 'Doors at ten.',
					date: '2026-12-05',
					start: '23:00',
					end: '06:00',
					tickets: '',
					audience: 'public'
				}
			}
		}
	});
	expect(await post('save', HOST, path, form(rkey, { name: '' }))).toMatchObject({
		status: 400,
		data: { error: 'Give the event a name', field: { name: 'name' } }
	});
	expect(await post('save', HOST, path, form(rkey, { performer: ['', '@nobody.test'] }))).toMatchObject({
		status: 400,
		data: { error: "There's no account called nobody.test", field: { name: 'performer', row: 1 } }
	});
	expect(await post('save', DOOR, path, form(rkey, { name: 'Taken over' }))).toMatchObject({ status: 403 });
});

it('keeps a draft without a venue on its own time zone, which its editor lists and names', async () => {
	const rkey = await make({ venue: undefined, publish: false });
	const path = `/party/${key}/${rkey}/winter-opening`;

	// Nothing says where the party or its host are, so it starts on UTC.
	expect(stored(rkey).value).toMatchObject({ timeZone: 'UTC', startsAt: '2026-12-05T23:00:00Z' });
	expect(await opened(HOST, path)).toMatchObject({ event: { venue: null, timeZone: 'UTC' }, timeZones: [] });
	const { timeZones } = await opened(HOST, `${path}?edit`);
	expect(timeZones?.[0]).toBe('UTC');
	expect(timeZones).toContain('Europe/Berlin');

	const saving = { ...form(rkey), venue: '', timeZone: 'UTC', description: 'Doors at eleven.' };
	await expect(post('save', HOST, `${path}?edit`, saving)).rejects.toMatchObject({ status: 303 });
	expect(stored(rkey).value).toMatchObject({ timeZone: 'UTC', description: 'Doors at eleven.' });
});

it('asks how the times move when the draft goes onto another clock, and saves each answer', async () => {
	const rkey = await make({ venue: undefined, publish: false });
	const path = `/party/${key}/${rkey}/winter-opening?edit`;
	const toBerlin = { ...form(rkey), venue: '', timeZone: 'Europe/Berlin' };

	expect(await post('save', HOST, path, toBerlin)).toMatchObject({
		status: 400,
		data: {
			error: 'Pick which times to keep',
			field: { name: 'zoneChange', row: 0 },
			values: { timeZone: 'Europe/Berlin' },
			choice: {
				name: 'zoneChange',
				question:
					'The times move to Europe/Berlin. Keep 23:00 on its clock (23:00 Berlin), or keep the same moment (00:00 Berlin the next day)?',
				options: [
					{ value: 'clock', label: 'Keep 23:00 on its clock' },
					{ value: 'instant', label: 'Keep the same moment' }
				]
			}
		}
	});
	expect(stored(rkey).value).toMatchObject({ timeZone: 'UTC' });

	await expect(post('save', HOST, path, { ...toBerlin, zoneChange: 'instant' })).rejects.toMatchObject({ status: 303 });
	expect(stored(rkey).value).toMatchObject({
		timeZone: 'Europe/Berlin',
		startsAt: '2026-12-06T00:00:00+01:00',
		endsAt: '2026-12-06T07:00:00+01:00'
	});

	// Back to UTC, keeping 00:00 as entered on its clock this time.
	const toUtc = { ...form(rkey), venue: '', timeZone: 'UTC', date: '2026-12-06', start: '00:00', end: '07:00' };
	await expect(post('save', HOST, path, { ...toUtc, zoneChange: 'clock' })).rejects.toMatchObject({ status: 303 });
	expect(stored(rkey).value).toMatchObject({ timeZone: 'UTC', startsAt: '2026-12-06T00:00:00Z' });
});

it("asks when a picked venue is on another clock, and nothing when it's on the draft's", async () => {
	const rkey = await make({ venue: undefined, publish: false });
	const path = `/party/${key}/${rkey}/winter-opening?edit`;

	expect(await post('save', HOST, path, form(rkey))).toMatchObject({
		status: 400,
		data: { choice: { question: expect.stringMatching(/^The venue is in Europe\/Berlin\. Keep 23:00 on its clock/) } }
	});

	const berlin = { ...form(rkey), venue: '', timeZone: 'Europe/Berlin' };
	await expect(post('save', HOST, path, { ...berlin, zoneChange: 'clock' })).rejects.toMatchObject({ status: 303 });
	await expect(post('save', HOST, path, form(rkey))).rejects.toMatchObject({ status: 303 });
	expect(stored(rkey).value).toMatchObject({ venue: venue.space, startsAt: '2026-12-05T23:00:00+01:00' });
	expect(stored(rkey).value).not.toHaveProperty('timeZone');
});

it('cancels a published event over the version its page shows', async () => {
	const rkey = await make();
	const path = `/party/${key}/${rkey}/winter-opening`;
	const before = stored(rkey).cid;

	expect(await post('cancel', HOST, path, { cid: before })).toBeUndefined();
	expect(stored(rkey).value).toMatchObject({ status: EVENT_STATUS.cancelled });
	expect(await post('cancel', HOST, path, { cid: before })).toMatchObject({ status: 409 });
	expect(await post('cancel', DOOR, path, { cid: stored(rkey).cid })).toMatchObject({ status: 403 });
	await expect(post('cancel', HOST, `/party/${key}/night-shift`, { cid: before })).rejects.toMatchObject({
		status: 404
	});
});

it("resets a private event's link, so the old one stops opening it", async () => {
	const rkey = await make({ audience: 'private' });
	const path = `/party/${key}/${rkey}/winter-opening`;
	const { linkToken } = (await t.events.get(uriOf(rkey)))!.record;

	expect(await post('resetLink', HOST, path, {})).toBeUndefined();
	await expect(opened(null, `${path}?t=${linkToken}`)).rejects.toMatchObject({ status: 404 });
	expect(await post('resetLink', DOOR, path, {})).toMatchObject({ status: 403 });
	await expect(post('resetLink', HOST, `/party/${key}`, {})).rejects.toMatchObject({ status: 404 });
});

it("lists the party's upcoming events: public ones for visitors, and drafts, private ones and ones never confirmed too for its people", async () => {
	const winter = await make();
	const hidden = await make({ name: 'Back Room', audience: 'private', date: '2026-12-12' });
	const draft = await make({ name: 'Spring', publish: false, date: undefined, start: undefined, end: undefined });
	const never = await make({ name: 'Last Week', date: '2026-11-01' });
	const past = await make({ name: 'Confirmed Last Week', date: '2026-11-02' });
	const owner = 'did:plc:owner';
	await t.sharedSpaces.addPerson(await t.viewer(ADMIN), venue.space, { did: owner, role: 'owner' });
	await t.events.approve(await t.viewer(owner), uriOf(past));
	const path = `/party/${key}/night-shift`;

	expect((await opened(STRANGER, path)).events).toEqual([
		{
			href: `/party/${key}/${winter}/winter-opening`,
			name: 'Winter Opening',
			startsAt: '2026-12-05T23:00:00+01:00',
			status: EVENT_STATUS.scheduled,
			venue: 'Tresor',
			draft: false,
			private: false,
			neverConfirmed: false
		}
	]);
	expect(
		(await opened(DOOR, path)).events?.map(({ href, draft, private: isPrivate, neverConfirmed }) => ({
			href,
			draft,
			isPrivate,
			neverConfirmed
		}))
	).toEqual([
		{ href: `/party/${key}/${never}/last-week`, draft: false, isPrivate: false, neverConfirmed: true },
		{ href: `/party/${key}/${winter}/winter-opening`, draft: false, isPrivate: false, neverConfirmed: false },
		{ href: `/party/${key}/${hidden}/back-room`, draft: false, isPrivate: true, neverConfirmed: false },
		{ href: `/party/${key}/${draft}/spring`, draft: true, isPrivate: false, neverConfirmed: false }
	]);
	expect(await opened(HOST, path)).toMatchObject({ newEventHref: `/party/${key}/new` });
	expect(await opened(DOOR, path)).toMatchObject({ newEventHref: null });
});

it("keeps an event never confirmed to its party's people, marked, and a past confirmed one open to anyone", async () => {
	const never = await make({ date: '2026-11-01' });
	const path = `/party/${key}/${never}/winter-opening`;

	await expect(opened(STRANGER, path)).rejects.toMatchObject({ status: 404 });
	expect(await opened(DOOR, path)).toMatchObject({ event: { confirmed: false, neverConfirmed: true } });

	const past = await make({ date: '2026-11-02' });
	const owner = 'did:plc:owner';
	await t.sharedSpaces.addPerson(await t.viewer(ADMIN), venue.space, { did: owner, role: 'owner' });
	await t.events.approve(await t.viewer(owner), uriOf(past));

	expect(await opened(STRANGER, `/party/${key}/${past}/winter-opening`)).toMatchObject({
		event: { confirmed: true, neverConfirmed: false }
	});
});
