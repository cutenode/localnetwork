import { afterEach, beforeEach, expect, it } from 'vitest';
import type { EpkView } from '#lib/epk.ts';
import { CODJ, DJ, FRIEND, pressKitServices, PROMOTER, STRANGER } from '#lib/server/testing/press-kits.ts';
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

/** The access page's path for the press kit `rkey`. */
const path = (rkey: string) => `/dashboard/projects/${f.project.slug}/press-kits/${rkey}/access`;

/** What the access page loads for `did`. */
async function open(did: string, rkey: string) {
	return loadRoute<PageServerData>(load, {
		app: f.t,
		viewer: await f.t.viewer(did),
		url: path(rkey),
		params: { slug: f.project.slug, rkey }
	});
}

/** What the access page's `action` returns for `did` on `rkey` with `form`. */
const submit = async (
	action: 'approve' | 'decline' | 'revoke' | 'invite',
	did: string,
	rkey: string,
	form: Record<string, string>
) =>
	callRoute(actions[action], {
		app: f.t,
		viewer: await f.t.viewer(did),
		url: `${path(rkey)}?/${action}`,
		params: { slug: f.project.slug, rkey },
		form
	});

/** PROMOTER's request to see `epk`, waiting for a manager. */
async function requested(epk: EpkView) {
	await f.t.epks.request(await f.t.viewer(PROMOTER), f.kit(epk), {
		role: 'promoter',
		message: 'For our winter series'
	});
	const [request] = await f.t.epks.requests(epk);

	return request;
}

/** Where `did` stands with `epk`. */
const access = async (epk: EpkView, did: string) =>
	(await f.t.epks.accessState(f.kit(epk), await f.t.viewer(did))).kind;

it("lists the press kit's requests and the people let in", async () => {
	const epk = await f.publish(DJ);
	await requested(epk);
	await f.t.epks.invite(await f.t.viewer(DJ), f.kit(epk), FRIEND);

	const data = await open(CODJ, epk.rkey);

	expect(data).toMatchObject({ epk: { rkey: epk.rkey }, legacyMaker: null });
	expect(data.requests).toMatchObject([{ requester: { did: PROMOTER }, status: 'pending', role: 'promoter' }]);
	expect(data.grants).toMatchObject([{ subject: { did: FRIEND }, via: 'invited' }]);
});

it("refuses someone who doesn't manage the project", async () => {
	const epk = await f.publish(DJ);

	await expect(open(STRANGER, epk.rkey)).rejects.toMatchObject({ status: 403 });
});

it('approves a request, which lets its asker in', async () => {
	const epk = await f.publish(DJ);
	const request = await requested(epk);

	const result = await submit('approve', CODJ, epk.rkey, { request: request.uri });

	expect(result).toEqual({ success: 'Approved. They can see the whole press kit now.' });
	expect(await access(epk, PROMOTER)).toBe('granted');
});

it('declines a request', async () => {
	const epk = await f.publish(DJ);
	const request = await requested(epk);

	const result = await submit('decline', DJ, epk.rkey, { request: request.uri });

	expect(result).toEqual({ success: 'Declined.' });
	expect(await access(epk, PROMOTER)).toBe('declined');
});

it('says when a request was already answered', async () => {
	const epk = await f.publish(DJ);
	const request = await requested(epk);
	await submit('decline', DJ, epk.rkey, { request: request.uri });

	expect(await submit('approve', CODJ, epk.rkey, { request: request.uri })).toMatchObject({
		status: 400,
		data: { error: 'That request has already been answered' }
	});
	expect(await submit('decline', CODJ, epk.rkey, { request: request.uri })).toMatchObject({ status: 400 });
});

it("takes someone's access away", async () => {
	const epk = await f.publish(DJ);
	await f.t.epks.invite(await f.t.viewer(DJ), f.kit(epk), FRIEND);

	const result = await submit('revoke', CODJ, epk.rkey, { subject: FRIEND });

	expect(result).toEqual({ success: 'Access removed.' });
	expect(await access(epk, FRIEND)).toBe('canAsk');
});

it("won't change access to a press kit still in its maker's repo", async () => {
	const legacy = await f.legacy(DJ);

	expect(await submit('revoke', CODJ, legacy.rkey, { subject: FRIEND })).toMatchObject({ status: 400 });
});

it('lets someone in by their DID', async () => {
	const epk = await f.publish(DJ);

	const result = await submit('invite', DJ, epk.rkey, { handle: FRIEND });

	expect(result).toEqual({ success: `Added @${FRIEND}.` });
	expect(await access(epk, FRIEND)).toBe('granted');
});

it('asks for a handle, and says when it finds nobody by one, keeping what was typed', async () => {
	const epk = await f.publish(DJ);

	expect(await submit('invite', DJ, epk.rkey, { handle: ' ' })).toMatchObject({
		status: 400,
		data: { error: 'Enter a handle', values: { handle: '' } }
	});
	expect(await submit('invite', DJ, epk.rkey, { handle: 'nobody.test' })).toMatchObject({
		status: 400,
		data: { error: "Couldn't find @nobody.test", values: { handle: 'nobody.test' } }
	});
});
