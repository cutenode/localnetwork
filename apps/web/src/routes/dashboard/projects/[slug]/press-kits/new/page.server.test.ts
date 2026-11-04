import { afterEach, beforeEach, expect, it } from 'vitest';
import { DJ, pressKitServices, STRANGER } from '#lib/server/testing/press-kits.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { actions, load } from './+page.server.ts';

let f: Awaited<ReturnType<typeof pressKitServices>>;

beforeEach(async () => {
	f = await pressKitServices();
});

afterEach(async () => {
	await f.t.audit.idle();
	await f.t.db.destroy();
});

/** A new press kit's form, filled in as a manager would. */
const form = { title: 'Festivals', headline: 'Big rooms, long sets', preset: 'review', longBio: 'The long bio' };

/** What `action` does with `fields` as `did`: what it returns, or the redirect it rejects with. */
const submit = async (action: 'save' | 'publish', did: string, fields: Record<string, string>) =>
	callRoute(actions[action], {
		app: f.t,
		viewer: await f.t.viewer(did),
		url: `/dashboard/projects/${f.project.slug}/press-kits/new?/${action}`,
		params: { slug: f.project.slug },
		form: fields
	});

it('opens an empty form that shows every project mix to everyone', async () => {
	const mixes = [{ title: 'Boiler Room', url: 'https://soundcloud.com/neon/boiler-room' }];
	const project = { ...f.project, profile: { ...f.project.profile, mixes } };

	const data = await loadRoute<PageServerData>(load, { parent: { project } });

	expect(data.values).toMatchObject({
		cid: '',
		title: '',
		mixes: [{ title: 'Boiler Room', url: 'https://soundcloud.com/neon/boiler-room', visibility: 'public' }]
	});
});

it('opens with no mixes for a project without any', async () => {
	const data = await loadRoute<PageServerData>(load, { parent: { project: f.project } });

	expect(data.values.mixes).toEqual([]);
});

it("saves the form as the manager's draft and opens it", async () => {
	const saving = submit('save', DJ, form);
	await expect(saving).rejects.toMatchObject({ status: 303 });
	const { drafts } = await f.t.epks.dashboard(f.project);

	expect(drafts.map((draft) => ({ author: draft.author.did, title: draft.changes.title }))).toEqual([
		{ author: DJ, title: 'Festivals' }
	]);
	// The draft's key is only known once it's saved.
	await expect(saving).rejects.toMatchObject({
		location: `/dashboard/projects/${f.project.slug}/press-kits/${drafts[0].rkey}?notice=saved`
	});
});

it('publishes the form and opens the press kit under the key it was published as', async () => {
	const publishing = submit('publish', DJ, form);
	await expect(publishing).rejects.toMatchObject({ status: 303 });
	const [epk] = await f.t.epks.forProject(f.project);

	expect(epk.record).toMatchObject({ title: 'Festivals', headline: 'Big rooms, long sets' });
	// The press kit's key is only known once it's published.
	await expect(publishing).rejects.toMatchObject({
		location: `/dashboard/projects/${f.project.slug}/press-kits/${epk.rkey}?notice=published`
	});
});

it('gives the form back with what was wrong when it has no name', async () => {
	const result = await submit('publish', DJ, { ...form, title: '' });

	expect(result).toMatchObject({
		status: 400,
		data: {
			error: 'Give the press kit a name, like “Clubs” or “Festivals”',
			values: { title: '', headline: 'Big rooms, long sets', longBio: 'The long bio' }
		}
	});
	expect(await f.t.epks.dashboard(f.project)).toEqual({ kits: [], drafts: [] });
});

it("refuses someone who doesn't manage the project", async () => {
	await expect(submit('save', STRANGER, form)).rejects.toMatchObject({ status: 403 });
});
