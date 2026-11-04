import { afterEach, beforeEach, expect, it } from 'vitest';
import type { EpkView } from '#lib/epk.ts';
import { CODJ, DJ, pressKitServices, PROMOTER, STRANGER, values } from '#lib/server/testing/press-kits.ts';
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

/** The editor's path for the press kit `rkey`. */
const editor = (rkey: string) => `/dashboard/projects/${f.project.slug}/press-kits/${rkey}`;

/** The editor's form for `epk` as it opens on the published version, with `fields` changed. */
const kitForm = (epk: EpkView, fields: Record<string, string | string[]> = {}) => ({
	cid: epk.cid,
	title: epk.record.title,
	headline: epk.record.headline ?? '',
	preset: 'review',
	longBio: 'The long bio',
	...fields
});

/** What the editor of `rkey` loads for `did`, with `query`. */
async function open(did: string, rkey: string, query = '') {
	return loadRoute<PageServerData>(load, {
		app: f.t,
		viewer: await f.t.viewer(did),
		url: `${editor(rkey)}${query}`,
		params: { slug: f.project.slug, rkey }
	});
}

/** What the editor's `action` does for `did` on `rkey` with `form`: what it returns, or the redirect it rejects with. */
const submit = async (
	action: 'save' | 'publish' | 'resolve' | 'discard' | 'delete',
	did: string,
	rkey: string,
	form: Record<string, string | string[]> = {}
) =>
	callRoute(actions[action], {
		app: f.t,
		viewer: await f.t.viewer(did),
		url: `${editor(rkey)}?/${action}`,
		params: { slug: f.project.slug, rkey },
		form
	});

/** DJ's draft of `epk` with its headline changed, and CODJ's headline published over it since. */
async function conflicting(epk: EpkView) {
	await f.t.epks.drafts.save(await f.t.viewer(DJ), f.project, {
		rkey: epk.rkey,
		cid: epk.cid,
		values: values({ headline: "DJ's headline" })
	});

	return f.publish(CODJ, { headline: "CODJ's headline" }, epk.rkey);
}

it('opens a published press kit with its requests waiting and the notice it came back with', async () => {
	const epk = await f.publish(DJ);
	await f.t.epks.request(await f.t.viewer(PROMOTER), f.kit(epk), { role: 'promoter' });

	const data = await open(DJ, epk.rkey, '?notice=published');

	expect(data).toMatchObject({
		epk: { rkey: epk.rkey },
		legacyMaker: null,
		title: 'Clubs',
		draft: null,
		others: [],
		values: { cid: epk.cid, title: 'Clubs', headline: 'Rolling techno for late rooms' },
		waiting: 1,
		notice: 'Published.'
	});
});

it("counts only the requests still waiting, and ignores a notice it doesn't know", async () => {
	const epk = await f.publish(DJ);
	await f.t.epks.request(await f.t.viewer(PROMOTER), f.kit(epk), {});
	const [request] = await f.t.epks.requests(epk);
	await f.t.epks.decide(await f.t.viewer(DJ), f.kit(epk), request.uri, 'decline');

	expect(await open(DJ, epk.rkey, '?notice=hacked')).toMatchObject({ waiting: 0, notice: null });
});

it("opens on the manager's own draft, and lists the other managers' drafts", async () => {
	const epk = await f.publish(DJ);
	const opened = { rkey: epk.rkey, cid: epk.cid };
	await f.t.epks.drafts.save(await f.t.viewer(CODJ), f.project, { ...opened, values: values({ headline: 'CODJ' }) });
	await f.t.epks.drafts.save(await f.t.viewer(DJ), f.project, { ...opened, values: values({ headline: 'Mine' }) });

	const data = await open(DJ, epk.rkey);

	expect(data).toMatchObject({ draft: { status: 'current' }, values: { headline: 'Mine' } });
	expect(data.others).toMatchObject([{ author: { did: CODJ } }]);
});

it("names a new press kit by its draft's name until it's published", async () => {
	const draft = await f.t.epks.drafts.save(await f.t.viewer(DJ), f.project, { values: values({ title: 'Festivals' }) });

	expect(await open(DJ, draft!.rkey)).toMatchObject({ epk: null, title: 'Festivals', values: { cid: '' }, waiting: 0 });
});

it('calls it a new press kit when its draft has no name of its own and the press kit was deleted', async () => {
	const epk = await f.publish(DJ);
	await f.t.epks.drafts.save(await f.t.viewer(DJ), f.project, {
		rkey: epk.rkey,
		cid: epk.cid,
		values: values({ headline: 'Changed' })
	});
	await f.t.epks.remove(await f.t.viewer(CODJ), f.kit(epk));

	expect(await open(DJ, epk.rkey)).toMatchObject({ epk: null, title: 'New press kit' });
});

it("says who has to move a press kit still in its maker's repo, without reading its requests", async () => {
	const legacy = await f.legacy(DJ);

	expect(await open(CODJ, legacy.rkey)).toMatchObject({ epk: { legacy: true }, legacyMaker: { did: DJ }, waiting: 0 });
});

it("answers a press kit that isn't there with a 404, and refuses someone who doesn't manage the project", async () => {
	await expect(open(DJ, '3lbnxyk5m2s2a')).rejects.toMatchObject({ status: 404 });

	const epk = await f.publish(DJ);

	await expect(open(STRANGER, epk.rkey)).rejects.toMatchObject({ status: 403 });
});

it('saves the form as a draft and opens the editor on it', async () => {
	const epk = await f.publish(DJ);

	await expect(submit('save', DJ, epk.rkey, kitForm(epk, { headline: 'Warehouse techno' }))).rejects.toMatchObject({
		status: 303,
		location: `${editor(epk.rkey)}?notice=saved`
	});
	expect(await open(DJ, epk.rkey)).toMatchObject({
		draft: { status: 'current' },
		values: { headline: 'Warehouse techno' }
	});
});

it('says nothing changed when the form matches the published press kit, for saving and publishing', async () => {
	const first = await f.publish(DJ);
	await expect(submit('publish', DJ, first.rkey, kitForm(first))).rejects.toMatchObject({ status: 303 });
	const [epk] = await f.t.epks.forProject(f.project);

	await expect(submit('save', DJ, epk.rkey, kitForm(epk))).rejects.toMatchObject({
		status: 303,
		location: `${editor(epk.rkey)}?notice=unchanged`
	});
	await expect(submit('publish', DJ, epk.rkey, kitForm(epk))).rejects.toMatchObject({
		status: 303,
		location: `${editor(epk.rkey)}?notice=unchanged`
	});
});

it('gives the form back with what was wrong, for saving and publishing', async () => {
	const epk = await f.publish(DJ);
	const form = kitForm(epk, { title: '', headline: 'Warehouse techno' });
	const error = 'Give the press kit a name, like “Clubs” or “Festivals”';

	expect(await submit('save', DJ, epk.rkey, form)).toMatchObject({
		status: 400,
		data: { error, values: { headline: 'Warehouse techno' } }
	});
	expect(await submit('publish', DJ, epk.rkey, form)).toMatchObject({
		status: 400,
		data: { error, values: { headline: 'Warehouse techno' } }
	});
});

it('publishes the form and opens the press kit with a notice', async () => {
	const epk = await f.publish(DJ);

	await expect(submit('publish', DJ, epk.rkey, kitForm(epk, { headline: 'Warehouse techno' }))).rejects.toMatchObject({
		status: 303,
		location: `${editor(epk.rkey)}?notice=published`
	});
	expect((await f.t.epks.get(f.project, epk.rkey))?.epk.record.headline).toBe('Warehouse techno');
});

it('shows the fields another manager published since the draft changed them, and publishes nothing', async () => {
	const epk = await conflicting(await f.publish(DJ));

	const result = await submit('publish', DJ, epk.rkey, kitForm(epk, { headline: "DJ's headline" }));

	expect(result).toEqual({
		conflicts: [
			{
				field: 'headline',
				label: 'Headline',
				published: "CODJ's headline",
				previous: 'Rolling techno for late rooms',
				draft: "DJ's headline"
			}
		]
	});
	expect((await f.t.epks.get(f.project, epk.rkey))?.epk.record.headline).toBe("CODJ's headline");
});

it("publishes over a conflict with the draft's value once the manager picks it", async () => {
	const epk = await conflicting(await f.publish(DJ));

	await expect(
		submit('resolve', DJ, epk.rkey, { conflict: ['headline', 'bogus'], overwrite: 'headline' })
	).rejects.toMatchObject({ status: 303, location: `${editor(epk.rkey)}?notice=published` });
	expect((await f.t.epks.get(f.project, epk.rkey))?.epk.record.headline).toBe("DJ's headline");
});

it('shows the conflicts again when the manager resolves none of them', async () => {
	const epk = await conflicting(await f.publish(DJ));

	const result = await submit('resolve', DJ, epk.rkey);

	expect(result).toMatchObject({ conflicts: [{ field: 'headline', published: "CODJ's headline" }] });
});

it('says there is no draft to resolve when the manager has none', async () => {
	const epk = await f.publish(DJ);

	expect(await submit('resolve', DJ, epk.rkey)).toMatchObject({
		status: 404,
		data: { error: 'That draft does not exist' }
	});
});

it('discards a draft of a published press kit and opens the published one', async () => {
	const epk = await f.publish(DJ);
	await expect(submit('save', DJ, epk.rkey, kitForm(epk, { headline: 'Warehouse techno' }))).rejects.toMatchObject({
		status: 303
	});

	await expect(submit('discard', DJ, epk.rkey)).rejects.toMatchObject({
		status: 303,
		location: `${editor(epk.rkey)}?notice=discarded`
	});
	expect(await open(DJ, epk.rkey)).toMatchObject({
		draft: null,
		values: { headline: 'Rolling techno for late rooms' }
	});
});

it('discards the draft of a new press kit and goes back to the list', async () => {
	const draft = await f.t.epks.drafts.save(await f.t.viewer(DJ), f.project, { values: values() });

	await expect(submit('discard', DJ, draft!.rkey)).rejects.toMatchObject({
		status: 303,
		location: `/dashboard/projects/${f.project.slug}/press-kits`
	});
	expect((await f.t.epks.dashboard(f.project)).drafts).toEqual([]);
});

it('asks the manager to sign in again when their PDS session to delete the draft with is gone', async () => {
	const draft = await f.t.epks.drafts.save(await f.t.viewer(DJ), f.project, { values: values() });
	f.t.capable.delete(DJ);

	expect(await submit('discard', DJ, draft!.rkey)).toMatchObject({
		status: 401,
		data: { error: 'Sign in again to change your drafts', reauth: true }
	});
});

it('deletes a press kit and goes back to the list', async () => {
	const epk = await f.publish(DJ);

	await expect(submit('delete', CODJ, epk.rkey)).rejects.toMatchObject({
		status: 303,
		location: `/dashboard/projects/${f.project.slug}/press-kits?deleted=1`
	});
	expect(await f.t.epks.get(f.project, epk.rkey)).toBeNull();
});

it("won't delete a press kit still in its maker's repo for another manager", async () => {
	const legacy = await f.legacy(DJ);

	expect(await submit('delete', CODJ, legacy.rkey)).toMatchObject({ status: 400 });
	expect(await f.t.epks.get(f.project, legacy.rkey)).not.toBeNull();
});
