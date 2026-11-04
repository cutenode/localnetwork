import { afterEach, beforeEach, expect, it } from 'vitest';
import { createTestServices } from '#lib/server/testing/core.ts';
import { callRoute } from '#lib/testing/route-event.ts';
import { actions, load } from './+page.server.ts';

const ADMIN = 'did:plc:admin';

const MOD = 'did:plc:mod';

const DJ = 'did:plc:dj';

let t: Awaited<ReturnType<typeof createTestServices>>;

beforeEach(async () => {
	t = await createTestServices({ admins: [ADMIN] });
	await t.staff.grant(await t.viewer(ADMIN), MOD, 'moderator');
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

/** The app with geocoding on and an identity resolver that knows handles as `<name>.test`. */
const app = () => ({
	...t,
	geocoder: { enabled: true },
	identity: {
		resolveIdentifier: async (identifier: string) => {
			if (identifier.startsWith('did:')) return identifier;

			if (identifier.endsWith('.test')) return `did:plc:${identifier.slice(0, -'.test'.length)}`;

			throw new Error(`no such handle ${identifier}`);
		}
	}
});

const neonTide = { name: 'Neon Tide', kind: 'solo', genres: 'techno, dub techno', city: 'Berlin', country: 'de' };

it('opens the form for an admin, saying whether places can be looked up', async () => {
	expect(await callRoute(load, { app: app(), viewer: await t.viewer(ADMIN), url: '/admin/projects/new' })).toEqual({
		geocoder: true
	});
});

it('refuses a moderator', async () => {
	const viewer = await t.viewer(MOD);

	await expect(callRoute(load, { app: app(), viewer, url: '/admin/projects/new' })).rejects.toMatchObject({
		status: 403
	});
});

it('creates a project with its managers by handle or DID, then opens it', async () => {
	await expect(
		callRoute(actions.default, {
			app: app(),
			viewer: await t.viewer(ADMIN),
			url: '/admin/projects/new',
			form: { ...neonTide, managers: 'dj.test,\n did:plc:promoter', bookingEmail: 'bookings@neontide.test' }
		})
	).rejects.toMatchObject({ status: 303, location: '/admin/projects/neon-tide?created=1' });

	const project = await t.projects.bySlug('neon-tide');

	expect(project?.managers).toEqual([DJ, 'did:plc:promoter']);
	expect(project?.profile.genres).toEqual(['techno', 'dub techno']);
	expect(await t.projects.details(project!.uri)).toEqual({ bookingEmail: 'bookings@neontide.test' });
});

it('keeps what was typed when the form is incomplete', async () => {
	const result = await callRoute(actions.default, {
		app: app(),
		viewer: await t.viewer(ADMIN),
		url: '/admin/projects/new',
		form: { ...neonTide, genres: '' }
	});

	expect(result).toMatchObject({
		status: 400,
		data: { error: 'Add at least one genre', values: { name: 'Neon Tide', city: 'Berlin' } }
	});
	expect(await t.projects.bySlug('neon-tide')).toBeNull();
});
