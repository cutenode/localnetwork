import { afterEach, beforeEach, expect, it } from 'vitest';
import { showFormDefaults } from '#lib/shows.ts';
import type { ShowDetails } from '#lib/types.ts';
import { ADMIN, DJ, FRIEND, pressKitServices, PROMOTER, STRANGER } from '#lib/server/testing/press-kits.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { actions, load } from './+page.server.ts';

/** Next year, so the shows stay upcoming. Berlin is on UTC+1 in November. */
const YEAR = new Date().getUTCFullYear() + 1;

const BERLIN = { city: 'Berlin', country: 'DE', latE6: 52_520_000, lngE6: 13_405_000 };

/** The announce form, filled in. */
const form = {
	name: 'Tidal Hours',
	date: `${YEAR}-11-07`,
	start: '23:00',
	city: 'Berlin',
	country: 'DE',
	latE6: String(BERLIN.latE6),
	lngE6: String(BERLIN.lngE6)
};

/** The show a promoter's inquiry asks about. */
const asked: ShowDetails = {
	title: 'Rooftop Rave',
	date: `${YEAR}-12-12T23:30:00+01:00`,
	venue: 'The Lot',
	location: BERLIN
};

let f: Awaited<ReturnType<typeof pressKitServices>>;

beforeEach(async () => {
	f = await pressKitServices();
});

afterEach(async () => {
	await f.t.audit.idle();
	await f.t.db.destroy();
});

/** The app with the geocoder off. */
const app = () => ({ ...f.t, geocoder: { enabled: false } });

/** The announce form's load for DJ at `rkey` (`new` for a new show), with `query`, under the layout that found the project. */
const loaded = async (rkey: string, query = '') =>
	loadRoute<PageServerData>(load, {
		app: app(),
		viewer: await f.t.viewer(DJ),
		url: `/dashboard/projects/${f.project.slug}/shows/${rkey}${query}`,
		params: { slug: f.project.slug, rkey },
		parent: { project: f.project }
	});

/** What `did` sends with the `action` at the show `rkey`: the failure it comes back with, or the redirect it rejects with. */
const send = async (action: 'save' | 'delete', did: string, rkey: string, fields: Record<string, string> = {}) =>
	callRoute(actions[action], {
		app: app(),
		viewer: await f.t.viewer(did),
		url: `/dashboard/projects/${f.project.slug}/shows/${rkey}?/${action}`,
		params: { slug: f.project.slug, rkey },
		form: fields
	});

/** DJ announces a show for the project, and its key. */
async function announced() {
	await expect(send('save', DJ, 'new', form)).rejects.toMatchObject({ status: 303 });
	const [show] = await f.t.shows.forProject(f.project);

	return show.uri.split('/').at(-1)!;
}

/** PROMOTER's inquiry to `project` about the show they ask about, answered and confirmed unless `confirm` is false. */
async function booking(project = f.project, confirm = true) {
	const promoter = await f.t.viewer(PROMOTER);
	const space = await f.t.bookings.start({ actor: promoter, projectUri: project.uri, show: asked, message: 'Free?' });

	if (confirm) {
		await f.t.bookings.message({ actor: await f.t.viewer(project.managers[0]), space, text: 'Yes' });
		await f.t.bookings.setStatus({ actor: promoter, space, status: 'confirmed' });
	}

	return space.split('/').at(-1)!;
}

/** Another active project, managed by `managers`. */
async function otherProject(managers: string[]) {
	const admin = await f.t.viewer(ADMIN);

	const created = await f.t.projectAdmin.create({
		actor: admin,
		profile: { name: 'Low Hum', kind: 'solo', genres: ['house'], location: { city: 'Berlin', country: 'DE' } }
	});

	return f.t.projectAdmin.setManagers({ actor: admin, project: created, managers });
}

it('opens an empty form for a new show', async () => {
	expect(await loaded('new')).toEqual({ isNew: true, values: showFormDefaults, geocoder: false });
});

it('fills a new show in from the confirmed booking it was opened from', async () => {
	const skey = await booking();

	expect((await loaded('new', `?booking=${skey}`)).values).toEqual({
		...showFormDefaults,
		name: 'Rooftop Rave',
		date: `${YEAR}-12-12`,
		start: '23:30',
		venue: 'The Lot',
		city: 'Berlin',
		country: 'DE',
		latE6: String(BERLIN.latE6),
		lngE6: String(BERLIN.lngE6)
	});
});

it("opens an empty form from a booking that isn't confirmed, isn't there, or isn't this project's", async () => {
	const open = await booking(f.project, false);
	const elsewhere = await booking(await otherProject([DJ]));
	const notTheirs = await booking(await otherProject([FRIEND]));

	for (const skey of [open, elsewhere, notTheirs, 'missing'])
		expect((await loaded('new', `?booking=${skey}`)).values).toEqual(showFormDefaults);
});

it('opens an empty form from a confirmed booking whose inquiry is gone', async () => {
	const skey = await booking();
	// What the sync does when the promoter deletes their inquiry record.
	await f.t.db.deleteFrom('bookingEvent').where('kind', '=', 'inquiry').execute();

	expect((await loaded('new', `?booking=${skey}`)).values).toEqual(showFormDefaults);
});

it('opens a show the manager announced, and 404s for one that is not there', async () => {
	const rkey = await announced();

	expect(await loaded(rkey)).toMatchObject({ isNew: false, values: { name: 'Tidal Hours', start: '23:00' } });
	await expect(loaded('3m2ixkcvzq22c')).rejects.toMatchObject({ status: 404 });
});

it("announces a show and goes back to the project's shows", async () => {
	await expect(send('save', DJ, 'new', { ...form, venue: 'Halle Nord' })).rejects.toMatchObject({
		status: 303,
		location: `/dashboard/projects/${f.project.slug}#shows`
	});

	expect((await f.t.shows.forProject(f.project)).map((show) => show.record.name)).toEqual(['Tidal Hours']);
});

it('edits a show from the versions its form opened on', async () => {
	const rkey = await announced();
	const values = (await loaded(rkey)).values;

	await expect(
		send('save', DJ, rkey, {
			...form,
			name: 'Tidal Hours (sold out)',
			showCid: values.showCid,
			eventCid: values.eventCid
		})
	).rejects.toMatchObject({ status: 303 });
	expect((await loaded(rkey)).values.name).toBe('Tidal Hours (sold out)');
});

it('keeps what was typed, the cancelled box too, and says what to fix', async () => {
	expect(await send('save', DJ, 'new', { ...form, name: '', cancelled: 'on' })).toMatchObject({
		status: 400,
		data: { error: 'Give the show a name', values: { date: form.date, cancelled: true } }
	});
	expect(await send('save', DJ, 'new', { ...form, start: '' })).toMatchObject({
		status: 400,
		data: { error: 'Pick a date and start time', values: { cancelled: false } }
	});
	expect(await f.t.shows.forProject(f.project)).toEqual([]);
});

it("deletes a show, and says when there's no such show", async () => {
	const rkey = await announced();

	await expect(send('delete', DJ, rkey)).rejects.toMatchObject({
		status: 303,
		location: `/dashboard/projects/${f.project.slug}#shows`
	});
	expect(await f.t.shows.forProject(f.project)).toEqual([]);

	expect(await send('delete', DJ, rkey)).toMatchObject({ status: 404, data: { error: 'That show does not exist' } });
});

it("refuses to announce or delete for someone who doesn't manage the project", async () => {
	const rkey = await announced();

	const refused = { status: 403, body: { message: "Only this project's managers can manage it" } };

	await expect(send('save', STRANGER, 'new', form)).rejects.toMatchObject(refused);
	await expect(send('delete', STRANGER, rkey)).rejects.toMatchObject(refused);
	expect(await f.t.shows.forProject(f.project)).toHaveLength(1);
});
