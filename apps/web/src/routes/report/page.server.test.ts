import { afterEach, beforeEach, expect, it } from 'vitest';
import { OptionalScopeUnsupported, type OptionalScope, type SignedInAccount } from '#lib/server/atproto/oauth.ts';
import { createTestServices } from '#lib/server/testing/core.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { actions, load } from './+page.server.ts';

const DJ = 'did:plc:dj';

const FIREFOX = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:144.0) Gecko/20100101 Firefox/144.0';

/** A started authorization, as the stand-in PDS saw it. */
type Started = { identifier: string; state: string; account?: SignedInAccount; optional?: OptionalScope };

/** An authorization server that is `up`, refuses the optional scopes (`refusesBugReports`), or can't be reached. */
class FakeOAuth {
	readonly started: Started[] = [];

	constructor(private readonly server: 'up' | 'refusesBugReports' | 'down' = 'up') {}

	async authorize(
		identifier: string,
		state: string,
		options: { account?: SignedInAccount; optional?: OptionalScope } = {}
	): Promise<URL> {
		if (this.server === 'down') throw new Error('fetch failed');

		if (this.server === 'refusesBugReports' && options.optional) throw new OptionalScopeUnsupported(options.optional);
		this.started.push({ identifier, state, ...options });

		return new URL('https://pds.test/oauth/authorize?request_uri=urn:request:1');
	}
}

/** userinput.app, through the accounts that approved posting to it. A post fails while it's `down`. */
class FakeBugReports {
	readonly posts: { did: string; title: string; body: string }[] = [];
	down = false;

	constructor(
		readonly enabled = true,
		private readonly approved = [DJ]
	) {}

	async canPost(did: string): Promise<boolean> {
		return this.approved.includes(did);
	}

	async post(did: string, report: { title: string; body: string }): Promise<string> {
		if (this.down) throw new Error('userinput.app space is missing');
		this.posts.push({ did, ...report });

		return `https://userinput.app/d/${did}/3lreport${this.posts.length}`;
	}
}

let t: Awaited<ReturnType<typeof createTestServices>>;

beforeEach(async () => {
	t = await createTestServices();
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

/** The app with `oauth` and `bugReports`. */
const app = (bugReports = new FakeBugReports(), oauth = new FakeOAuth()) => ({ ...t, oauth, bugReports });

it('shows the form, with what the report will say about the page and browser', async () => {
	const data = await loadRoute<PageServerData>(load, {
		app: app(),
		viewer: await t.viewer(DJ),
		url: '/report?from=/shows',
		headers: { 'user-agent': FIREFOX }
	});

	expect(data).toEqual({
		facts: { page: '/shows', browser: 'Firefox 144, macOS' },
		github: expect.stringContaining('https://github.com/cutenode/localnetwork/issues/new?template=web-bug.yml'),
		canPost: true
	});
});

it('leaves the page out when it came from somewhere else', async () => {
	const data = await loadRoute<PageServerData>(load, {
		app: app(new FakeBugReports(true, [])),
		viewer: await t.viewer(DJ),
		url: '/report?from=https://evil.test'
	});

	expect(data).toMatchObject({ facts: { page: undefined, browser: undefined }, canPost: false });
});

it('sends someone signed out, or a site with no board to post to, to GitHub', async () => {
	await expect(callRoute(load, { app: app(), url: '/report?from=/shows' })).rejects.toMatchObject({
		status: 303,
		location: expect.stringMatching(/^https:\/\/github\.com\/cutenode\/localnetwork\/issues\/new\?.*Open\+%2Fshows/)
	});

	await expect(
		callRoute(load, { app: app(new FakeBugReports(false)), viewer: await t.viewer(DJ), url: '/report' })
	).rejects.toMatchObject({ status: 303, location: expect.stringContaining('https://github.com/') });
});

it('sends the viewer to approve posting bug reports, then back to the report', async () => {
	const oauth = new FakeOAuth();

	await expect(
		callRoute(actions.allow, {
			app: app(undefined, oauth),
			viewer: await t.viewer(DJ),
			url: '/report?/allow&from=/shows',
			form: {}
		})
	).rejects.toMatchObject({ status: 303, location: 'https://pds.test/oauth/authorize?request_uri=urn:request:1' });

	expect(oauth.started).toEqual([
		{
			identifier: DJ,
			state: JSON.stringify({ next: '/report?from=%2Fshows' }),
			account: undefined,
			optional: 'bugReport'
		}
	]);
});

it('comes back to the bare report page when it was opened from nowhere in particular', async () => {
	const oauth = new FakeOAuth();

	await expect(
		callRoute(actions.allow, {
			app: app(undefined, oauth),
			viewer: await t.viewer(DJ),
			url: '/report?/allow',
			form: {}
		})
	).rejects.toMatchObject({ status: 303 });

	expect(oauth.started).toMatchObject([{ state: JSON.stringify({ next: '/report' }) }]);
});

it("says so when the viewer's server won't let apps post bug reports, or can't be reached", async () => {
	const viewer = await t.viewer(DJ);

	expect(
		await callRoute(actions.allow, {
			app: app(undefined, new FakeOAuth('refusesBugReports')),
			viewer,
			url: '/report?/allow',
			form: {}
		})
	).toMatchObject({ status: 400, data: { error: "Your account's server doesn't let apps post bug reports for you." } });

	expect(
		await callRoute(actions.allow, {
			app: app(undefined, new FakeOAuth('down')),
			viewer,
			url: '/report?/allow',
			form: {}
		})
	).toMatchObject({ status: 502, data: { error: "We couldn't reach your account's server. Try again in a minute." } });
});

it('sends someone signed out to sign in before approving or sending', async () => {
	await expect(callRoute(actions.allow, { app: app(), url: '/report?/allow', form: {} })).rejects.toMatchObject({
		status: 303,
		location: '/login?next=%2Freport%3F%2Fallow'
	});
	await expect(
		callRoute(actions.send, { app: app(), url: '/report?/send', form: { title: 'Broken' } })
	).rejects.toMatchObject({
		status: 303,
		location: '/login?next=%2Freport%3F%2Fsend'
	});
});

it('posts the report with the page and browser under it', async () => {
	const bugReports = new FakeBugReports();

	const result = await callRoute(actions.send, {
		app: app(bugReports),
		viewer: await t.viewer(DJ),
		url: '/report?/send&from=/shows',
		headers: { 'user-agent': FIREFOX },
		form: { title: '  Map is blank  ', body: 'The map on the shows page never loads.' }
	});

	expect(result).toEqual({ posted: 'https://userinput.app/d/did:plc:dj/3lreport1' });
	expect(bugReports.posts).toEqual([
		{
			did: DJ,
			title: 'Map is blank',
			body: 'The map on the shows page never loads.\n\nPage: /shows\nBrowser: Firefox 144, macOS'
		}
	]);
});

it('asks for a title, keeping what was typed', async () => {
	const result = await callRoute(actions.send, {
		app: app(),
		viewer: await t.viewer(DJ),
		url: '/report?/send',
		form: { title: ' ', body: 'Something broke.' }
	});

	expect(result).toMatchObject({
		status: 400,
		data: {
			error: 'Add a title.',
			values: { title: '', body: 'Something broke.' },
			github: expect.stringContaining('github.com')
		}
	});
});

it("offers GitHub instead when the report can't be posted", async () => {
	const bugReports = new FakeBugReports();
	bugReports.down = true;

	const result = await callRoute(actions.send, {
		app: app(bugReports),
		viewer: await t.viewer(DJ),
		url: '/report?/send',
		form: { title: 'Map is blank', body: 'Never loads.' }
	});

	expect(result).toMatchObject({
		status: 502,
		data: {
			error: "We couldn't post your report.",
			values: { title: 'Map is blank', body: 'Never loads.' },
			github: expect.stringContaining('title=Map+is+blank')
		}
	});
});
