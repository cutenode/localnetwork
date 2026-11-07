import { afterEach, beforeEach, expect, it } from 'vitest';
import { createTestServices } from '#lib/server/testing/core.ts';
import { callRoute, loadRoute, type RouteEventInit } from '#lib/testing/route-event.ts';
import type { ProjectView } from '#lib/types.ts';
import type { PageServerData } from './$types';
import { actions, load } from './+page.server.ts';

const ADMIN = 'did:plc:admin';

const MOD = 'did:plc:mod';

const DJ = 'did:plc:dj';

const PROMOTER = 'did:plc:promoter';

let t: Awaited<ReturnType<typeof createTestServices>>;

let project: ProjectView;

beforeEach(async () => {
	t = await createTestServices({ admins: [ADMIN] });
	const admin = await t.viewer(ADMIN);
	await t.staff.grant(admin, MOD, 'moderator');

	project = await t.projectAdmin.create({
		actor: admin,
		profile: { name: 'Neon Tide', kind: 'solo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } },
		details: { bookingEmail: 'bookings@neontide.test' },
		managers: [DJ]
	});
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

/** The app with geocoding off and an identity resolver that knows handles as `<name>.test`. */
const app = () => ({
	...t,
	sets: t.projects.sets,
	geocoder: { enabled: false },
	identity: {
		resolveIdentifier: async (identifier: string) => {
			if (identifier.endsWith('.test')) return `did:plc:${identifier.slice(0, -'.test'.length)}`;

			throw new Error(`no such handle ${identifier}`);
		}
	}
});

/** The event for the project page as `did` sees it, with `init` on top. */
const as = async (did: string, init: RouteEventInit = {}): Promise<RouteEventInit> => ({
	app: app(),
	viewer: await t.viewer(did),
	url: `/admin/projects/${project.slug}`,
	params: { slug: project.slug },
	...init
});

it('shows a moderator the project, its form values, managers, history, and sets, without verifications', async () => {
	const data = await loadRoute<PageServerData>(load, await as(MOD));

	expect(data.project.uri).toBe(project.uri);
	expect(data.values).toMatchObject({ name: 'Neon Tide', genres: 'techno', bookingEmail: 'bookings@neontide.test' });
	expect(data.managers).toEqual([{ did: DJ, handle: null }]);
	expect(data.trail.map((entry) => entry.action)).toEqual(['project.create']);
	expect(data.requests).toEqual([]);
	expect({ created: data.created, geocoder: data.geocoder, verifications: data.verifications }).toEqual({
		created: false,
		geocoder: false,
		verifications: null
	});
	expect(await data.sets).toEqual([]);
});

it("shows an admin where each manager stands with the root's verifications, and that the project was just created", async () => {
	const data = await loadRoute<PageServerData>(
		load,
		await as(ADMIN, { url: `/admin/projects/${project.slug}?created=1` })
	);

	expect(data.created).toBe(true);
	expect(await data.verifications).toEqual({ [DJ]: 'unverified' });
});

it("answers 404 for a project that isn't in the directory, on the page and every action", async () => {
	const missing = { url: '/admin/projects/gone', params: { slug: 'gone' } };

	await expect(loadRoute<PageServerData>(load, await as(MOD, missing))).rejects.toMatchObject({ status: 404 });

	for (const action of [actions.update, actions.status, actions.managers])
		await expect(callRoute(action, await as(ADMIN, { ...missing, form: {} }))).rejects.toMatchObject({
			status: 404,
			body: { message: 'Project not found' }
		});
});

it('saves and publishes an edit from an admin', async () => {
	const result = await callRoute(
		actions.update,
		await as(ADMIN, {
			form: { name: 'Neon Tide', tagline: 'Dub techno at dawn', genres: 'techno', city: 'Berlin', country: 'DE' }
		})
	);

	expect(result).toEqual({ success: 'Saved and published' });
	const saved = await t.projects.bySlug(project.slug);
	expect(saved?.profile.tagline).toBe('Dub techno at dawn');
	expect(saved?.revision).toBe(2);
	// A form without booking fields clears the details the admin left out.
	expect(await t.projects.details(project.uri)).toEqual({});
});

it('keeps what was typed when an edit is incomplete, and leaves edits to admins', async () => {
	const form = { name: 'Neon Tide', genres: 'techno', city: 'Berlin', country: 'Germany', removeAvatar: 'on' };

	expect(await callRoute(actions.update, await as(ADMIN, { form }))).toMatchObject({
		status: 400,
		data: { error: 'Use a two-letter country code (like US, GB, or DE)', values: { country: 'Germany' } }
	});
	await expect(callRoute(actions.update, await as(MOD, { form }))).rejects.toMatchObject({ status: 403 });
	expect((await t.projects.bySlug(project.slug))?.revision).toBe(1);
});

it('lets a moderator hide a project, with a reason', async () => {
	const result = await callRoute(actions.status, await as(MOD, { form: { status: 'hidden', reason: 'Spam' } }));

	expect(result).toEqual({ success: 'Status set to hidden' });
	expect((await t.projects.bySlug(project.slug))?.status).toBe('hidden');

	const [entry] = await t.audit.list({ action: 'project.status' });
	expect(entry.reason).toBe('Spam');
});

it("sets a status without a reason, refuses one it doesn't know, and leaves archiving to admins", async () => {
	expect(await callRoute(actions.status, await as(ADMIN, { form: { status: 'hidden' } }))).toEqual({
		success: 'Status set to hidden'
	});
	expect((await t.audit.list({ action: 'project.status' }))[0].reason).toBeNull();

	expect(await callRoute(actions.status, await as(MOD, { form: { status: 'deleted' } }))).toEqual({
		error: 'Unknown status'
	});
	expect(await callRoute(actions.status, await as(MOD, { form: { status: 'archived' } }))).toMatchObject({
		status: 403,
		data: { error: 'Only admins can archive projects' }
	});
	expect((await t.projects.bySlug(project.slug))?.status).toBe('hidden');
});

it('keeps the managers ticked, drops ones that were never managers, and adds one by handle', async () => {
	const result = await callRoute(
		actions.managers,
		await as(ADMIN, { form: { keep: [DJ, 'did:plc:stranger'], add: 'promoter.test' } })
	);

	expect(result).toEqual({ success: 'Managers updated' });
	expect((await t.projects.bySlug(project.slug))?.managers).toEqual([DJ, PROMOTER]);
});

it('removes every manager left unticked, and leaves managers to admins', async () => {
	await expect(callRoute(actions.managers, await as(MOD, { form: {} }))).rejects.toMatchObject({ status: 403 });
	expect(await callRoute(actions.managers, await as(ADMIN, { form: {} }))).toEqual({ success: 'Managers updated' });
	expect((await t.projects.bySlug(project.slug))?.managers).toEqual([]);
});
