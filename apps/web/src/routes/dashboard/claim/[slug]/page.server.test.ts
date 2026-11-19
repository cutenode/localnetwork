import { isRedirect } from '@sveltejs/kit';
import { afterEach, beforeEach, expect, it } from 'vitest';
import { ADMIN, DJ, pressKitServices, PROMOTER } from '#lib/server/testing/press-kits.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import { actions, load } from './+page.server.ts';
import type { PageServerData } from './$types';

let f: Awaited<ReturnType<typeof pressKitServices>>;

beforeEach(async () => {
	f = await pressKitServices();
});

afterEach(async () => {
	await f.t.audit.idle();
	await f.t.db.destroy();
});

/** The claim page for `slug`, opened by `did`. */
const opened = async (did: string, slug = f.project.slug) =>
	loadRoute<PageServerData>(load, {
		app: f.t,
		viewer: { ...(await f.t.viewer(did)), handle: 'promoter.test' },
		url: `/dashboard/claim/${slug}`,
		params: { slug }
	});

/** Files the claim form for `slug` as `did`. */
const claim = async (did: string, form: Record<string, string>, slug = f.project.slug) =>
	callRoute(actions.default, {
		app: f.t,
		viewer: await f.t.viewer(did),
		url: `/dashboard/claim/${slug}`,
		params: { slug },
		form
	});

/** Where `running` redirects to, as SvelteKit throws it, or null when it doesn't redirect. */
async function redirectOf(running: Promise<unknown>) {
	try {
		await running;
	} catch (thrown) {
		if (isRedirect(thrown)) return thrown;
		throw thrown;
	}

	return null;
}

it('opens the claim form for a project with who is claiming it', async () => {
	const data = await opened(PROMOTER);

	expect(data.project.uri).toBe(f.project.uri);
	expect(data.claimant).toEqual({ did: PROMOTER, handle: 'promoter.test' });
});

it("sends a manager to their project, and won't open a project that's missing or archived", async () => {
	const admin = await f.t.viewer(ADMIN);
	const archived = await f.t.projectAdmin.create({ actor: admin, profile: { ...f.project.profile, name: 'Gone' } });
	await f.t.projectAdmin.setStatus({ actor: admin, project: archived, status: 'archived' });

	await expect(opened(DJ)).rejects.toMatchObject({ status: 303, location: `/dashboard/projects/${f.project.slug}` });
	await expect(opened(PROMOTER, 'nobody')).rejects.toMatchObject({ status: 404 });
	await expect(opened(PROMOTER, archived.slug)).rejects.toMatchObject({
		status: 404,
		body: { message: 'Project not found' }
	});
});

it('files the claim for review and opens the request', async () => {
	const redirect = await redirectOf(
		claim(PROMOTER, {
			relationship: 'manager',
			evidence: 'I book their tours',
			links: 'neontide.test/contact',
			message: 'Happy to verify by email'
		})
	);

	const [request] = await f.t.requests.list({ requester: PROMOTER });

	expect(request).toMatchObject({
		kind: 'claim',
		status: 'pending',
		record: {
			claim: { relationship: 'manager', evidence: 'I book their tours', links: ['https://neontide.test/contact'] },
			message: 'Happy to verify by email'
		}
	});
	expect(redirect).toMatchObject({ status: 303, location: `/dashboard/account/requests/${request.rkey}?submitted=1` });
});

it('keeps what was typed when the claim is refused', async () => {
	const result = await claim(PROMOTER, { relationship: 'fan', evidence: 'I saw them live' });

	expect(result).toMatchObject({
		status: 400,
		data: {
			error: 'Choose how you are connected to the project',
			values: { relationship: 'fan', evidence: 'I saw them live' }
		}
	});
	expect(await f.t.requests.list({ requester: PROMOTER })).toEqual([]);
});

it("refuses a claim on a project that doesn't exist", async () => {
	await expect(claim(PROMOTER, { relationship: 'self', evidence: 'It is me' }, 'nobody')).rejects.toMatchObject({
		status: 404
	});
});
