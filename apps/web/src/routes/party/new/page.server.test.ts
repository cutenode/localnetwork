import { afterEach, beforeEach, expect, it } from 'vitest';
import { createTestServices } from '#lib/server/testing/core.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import type { Flags } from '#lib/types.ts';
import { actions, load } from './+page.server.ts';
import type { PageServerData } from './$types';

const ADA = 'did:plc:ada';

let t: Awaited<ReturnType<typeof createTestServices>>;

const on: Partial<Flags> = { spacePages: true };

const app = () => ({ ...t, geocoder: { enabled: true }, config: { publicUrl: 'https://localnetwork.test' } });

const opened = async (did: string | null, flags = on) =>
	loadRoute<PageServerData>(load, { app: app(), viewer: did ? await t.viewer(did) : null, url: '/party/new', flags });

const save = async (form: Record<string, string>) =>
	callRoute(actions.save, { app: app(), viewer: await t.viewer(ADA), url: '/party/new?/save', flags: on, form });

/** The one party started, by its key. */
const started = async () => {
	const { skey } = await t.db.selectFrom('party').select('skey').executeTakeFirstOrThrow();

	return { skey, party: await t.parties.bySkey(skey) };
};

beforeEach(async () => {
	t = await createTestServices();
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

it('opens an empty party page in edit mode, for someone signed in, while spacePages is on', async () => {
	expect(await opened(ADA)).toEqual({
		subject: { kind: 'party', uri: 'party:new' },
		can: expect.arrayContaining(['page.edit', 'people.manage']),
		role: 'owner',
		geocoder: true,
		meta: expect.objectContaining({ title: 'New party', noindex: true })
	});
	await expect(opened(ADA, {})).rejects.toMatchObject({ status: 404 });
	await expect(opened(null)).rejects.toMatchObject({ status: 303, location: '/login?next=%2Fparty%2Fnew' });
});

it('saves a draft and goes on to edit it at its own address', async () => {
	const saving = save({ name: 'Night Shift', bio: 'Techno on Thursdays.' });
	await expect(saving).rejects.toMatchObject({ status: 303 });
	const { skey, party } = await started();

	await expect(saving).rejects.toMatchObject({ location: `/party/${skey}/night-shift?edit` });
	expect(party).toMatchObject({ basics: { name: 'Night Shift', bio: 'Techno on Thursdays.' }, publishedAt: null });
});

it('starts and publishes in one go, to its page', async () => {
	const saving = save({ name: 'Night Shift', publish: 'on' });
	await expect(saving).rejects.toMatchObject({ status: 303 });
	const { skey, party } = await started();

	await expect(saving).rejects.toMatchObject({ location: `/party/${skey}/night-shift` });
	expect(party?.publishedAt).toEqual(expect.any(String));
});

it('needs a name, and keeps what was typed', async () => {
	expect(await save({ name: '', bio: 'Techno' })).toMatchObject({
		status: 400,
		data: { error: "Add the party's name", field: { name: 'name' }, values: { name: '', bio: 'Techno' } }
	});
	expect(await t.db.selectFrom('party').selectAll().execute()).toEqual([]);
});
