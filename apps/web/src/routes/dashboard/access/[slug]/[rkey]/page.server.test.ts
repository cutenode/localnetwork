import { afterEach, beforeEach, expect, it } from 'vitest';
import { ADMIN, CODJ, DJ, preset, pressKitServices, PROMOTER } from '#lib/server/testing/press-kits.ts';
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

/** The page's path, where someone asks to see the press kit `rkey` of the project `slug`. */
const path = (rkey: string, slug = f.project.slug) => `/dashboard/access/${slug}/${rkey}`;

/** What the page loads for `did` (signed out without one). */
const open = async (did: string | null, rkey: string, slug = f.project.slug) =>
	loadRoute<PageServerData>(load, {
		app: f.t,
		viewer: did ? await f.t.viewer(did) : null,
		url: path(rkey, slug),
		params: { slug, rkey }
	});

/** What asking for the press kit `rkey` does for `did`: what it returns, or the redirect it rejects with. */
const ask = async (did: string, rkey: string, form: Record<string, string> = {}) =>
	callRoute(actions.request, {
		app: f.t,
		viewer: await f.t.viewer(did),
		url: `${path(rkey)}?/request`,
		params: { slug: f.project.slug, rkey },
		form
	});

/** The project hidden by an admin. */
const hide = async () =>
	f.t.projectAdmin.setStatus({ actor: await f.t.viewer(ADMIN), project: f.project, status: 'hidden' });

it('shows someone where they stand with a press kit they can ask to see', async () => {
	const epk = await f.publish(DJ);

	expect(await open(PROMOTER, epk.rkey)).toMatchObject({
		epk: { rkey: epk.rkey },
		project: { slug: f.project.slug },
		access: { kind: 'canAsk', rule: 'review' }
	});
});

it("sends one of the project's managers to the page where they decide who gets in", async () => {
	const epk = await f.publish(DJ);

	await expect(open(CODJ, epk.rkey)).rejects.toMatchObject({
		status: 303,
		location: `/dashboard/projects/${f.project.slug}/press-kits/${epk.rkey}/access`
	});
});

it('sends someone signed out to sign in', async () => {
	const epk = await f.publish(DJ);

	await expect(open(null, epk.rkey)).rejects.toMatchObject({
		status: 303,
		location: `/login?next=${encodeURIComponent(path(epk.rkey))}`
	});
});

it("answers a press kit or a project that isn't there with a 404", async () => {
	await expect(open(PROMOTER, '3lbnxyk5m2s2a')).rejects.toMatchObject({ status: 404 });
	await expect(open(PROMOTER, '3lbnxyk5m2s2a', 'nobody')).rejects.toMatchObject({ status: 404 });
});

it("hides a hidden project's press kit from everyone but its managers and staff", async () => {
	const epk = await f.publish(DJ);
	await hide();

	await expect(open(PROMOTER, epk.rkey)).rejects.toMatchObject({ status: 404 });
	await expect(open(DJ, epk.rkey)).rejects.toMatchObject({ status: 303 });
	expect(await open(ADMIN, epk.rkey)).toMatchObject({ epk: { rkey: epk.rkey }, access: { kind: 'canAsk' } });
});

it('sends a request to the managers, and says it waits on them', async () => {
	const epk = await f.publish(DJ);

	const result = await ask(PROMOTER, epk.rkey, { role: 'promoter', message: 'For our winter series' });

	expect(result).toEqual({ requested: 'pending' });
	expect(
		(await f.t.epks.requests(epk)).map((request) => [request.requester.did, request.role, request.message])
	).toEqual([[PROMOTER, 'promoter', 'For our winter series']]);
});

it('opens the whole press kit straight away when its rules let the asker in', async () => {
	const epk = await f.publish(DJ, { access: preset('open') });

	await expect(ask(PROMOTER, epk.rkey)).rejects.toMatchObject({
		status: 303,
		location: `/projects/${f.project.slug}/press-kits/${epk.rkey}`
	});
});

it('says when someone already asked', async () => {
	const epk = await f.publish(DJ);
	await ask(PROMOTER, epk.rkey);

	expect(await ask(PROMOTER, epk.rkey)).toMatchObject({
		status: 400,
		data: { error: 'You already asked; the managers will see it' }
	});
});
