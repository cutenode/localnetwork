import { afterEach, beforeEach, expect, it } from 'vitest';
import { ensureRootSpaces } from '#lib/server/bootstrap.ts';
import { HappyViewClient, HappyViewError } from '#lib/server/happyview/client.ts';
import { createTestServices, ROOT } from '#lib/server/testing/core.ts';
import { callRoute, loadRoute, type RouteEventInit } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { actions, load } from './+page.server.ts';

const ADMIN = 'did:plc:admin';

const MOD = 'did:plc:mod';

let t: Awaited<ReturnType<typeof createTestServices>>;

beforeEach(async () => {
	t = await createTestServices({ admins: [ADMIN], capable: [ADMIN] });
	await t.staff.grant(await t.viewer(ADMIN), MOD, 'moderator');
	await t.audit.idle();
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

/** The admin signs in, which the audit log records and copies to the root's repo and theirs. */
const signIn = async () => {
	await t.audit.record({ actor: ADMIN, actorRole: 'admin', action: 'auth.login', summary: 'Signed in' });
	await t.audit.idle();
};

type DirectorySync = { at: string; error: string | null };

/**
 * The app as it runs with the root on its own PDS and HappyView answering, holding HappyView's API key when it
 * `managesHappyView`. Resyncing bootstraps the root's spaces for real; syncing the directory records `directory` as
 * how it went, the way the app keeps the last outcome.
 */
function systemApp(
	directory: DirectorySync = { at: '2026-11-02T12:00:00.000Z', error: null },
	managesHappyView = true
) {
	let directorySync: DirectorySync | null = null;

	return {
		...t,
		rootDid: ROOT,
		rootAccount: { handle: 'localnetwork.test', service: 'https://pds.localnetwork.test' },
		syncer: {
			status: async () => [{ space: t.spaces.audit, lastSyncedAt: '2026-11-02T11:59:00.000Z', lastError: null }]
		},
		happyView: new HappyViewClient({
			url: 'http://happyview.test',
			fetch: async (input) =>
				new Response('ok', { status: new URL(new Request(input).url).pathname === '/health' ? 200 : 404 })
		}),
		backupStatus: { lastAt: '2026-11-02T03:00:00.000Z', lastError: null },
		config: {
			publicUrl: 'https://localnetwork.test',
			serviceDid: 'did:web:localnetwork.test',
			syncNotify: true,
			syncPollIntervalMs: 60_000,
			geocoder: 'open-meteo',
			happyView: { url: 'http://happyview.test', apiKey: managesHappyView ? 'hv_test' : undefined }
		},
		get directorySync() {
			return directorySync;
		},
		bootstrap: () => ensureRootSpaces(t.core),
		syncDirectory: async () => {
			directorySync = directory;
		}
	};
}

/** The System page's event as `did` sees it, with `init` on top. */
const as = async (did: string, init: RouteEventInit = {}): Promise<RouteEventInit> => ({
	app: systemApp(),
	viewer: await t.viewer(did),
	url: '/admin/system',
	...init
});

it("shows an admin the root account, its spaces' policies, sync, HappyView, backups, and the config", async () => {
	const data = await loadRoute<PageServerData>(load, await as(ADMIN));

	expect(data.root).toEqual({ did: ROOT, handle: 'localnetwork.test', service: 'https://pds.localnetwork.test' });
	expect(data.spaces.map((space) => space.name)).toEqual(Object.keys(t.spaces));
	expect(data.spaces[0].uri).toBe(Object.values(t.spaces)[0]);
	expect(data.sync).toHaveLength(1);
	expect(data.audit).toMatchObject({ total: 6, pending: 0, failed: 0 });
	expect(data.failed).toEqual([]);
	expect(data.happyView).toEqual({ url: 'http://happyview.test', reachable: true, sync: null });
	expect(data.backups).toEqual({ lastAt: '2026-11-02T03:00:00.000Z', lastError: null });
	expect(data.config).toEqual({
		publicUrl: 'https://localnetwork.test',
		serviceDid: 'did:web:localnetwork.test',
		syncNotify: true,
		pollMs: 60_000,
		geocoder: 'open-meteo'
	});
});

it("lists audit entries whose copies haven't been written yet, newest first", async () => {
	t.net.failWrites(ROOT, 2);
	await signIn();
	await signIn();

	const { failed } = await loadRoute<PageServerData>(load, await as(ADMIN));

	expect(failed.map((entry) => [entry.seq, entry.rootStatus, entry.actorStatus])).toEqual([
		[8, 'pending', 'written'],
		[7, 'pending', 'written']
	]);
	expect(failed[0].rootError).toMatch(/UpstreamFailure/);
});

it("shows a root account whose handle and PDS aren't known yet", async () => {
	const app = { ...systemApp(), rootAccount: {} };

	expect((await loadRoute<PageServerData>(load, await as(ADMIN, { app }))).root).toEqual({
		did: ROOT,
		handle: null,
		service: null
	});
});

it('leaves the System page and its actions to admins', async () => {
	const init = await as(MOD, { form: {} });

	await expect(loadRoute<PageServerData>(load, init)).rejects.toMatchObject({ status: 403 });
	await expect(callRoute(actions.resync, init)).rejects.toMatchObject({ status: 403 });
	await expect(callRoute(actions.retryAudit, init)).rejects.toMatchObject({ status: 403 });
	await expect(callRoute(actions.planMigration, init)).rejects.toMatchObject({ status: 403 });
});

it('bootstraps and syncs every space and the directory', async () => {
	const app = systemApp();
	const industry = t.net.spaces.get(t.spaces.industry)!;
	industry.policy = { ...industry.policy, publicRead: !industry.policy.publicRead };

	expect(await callRoute(actions.resync, await as(ADMIN, { app, form: {} }))).toEqual({
		success: 'Bootstrapped and synced every space and the directory'
	});
	expect(app.directorySync).toEqual({ at: '2026-11-02T12:00:00.000Z', error: null });
	expect((await t.audit.list({ action: 'system.space' }))[0].summary).toBe('Corrected the industry space policies');
});

it("leaves the directory out of what it synced when this app doesn't manage HappyView", async () => {
	const app = systemApp(undefined, false);

	expect(await callRoute(actions.resync, await as(ADMIN, { app, form: {} }))).toEqual({
		success: 'Bootstrapped and synced every space'
	});
});

it("says when the spaces synced but HappyView didn't", async () => {
	const app = systemApp({ at: '2026-11-02T12:00:00.000Z', error: 'lexicon upload failed: 401' });

	expect(await callRoute(actions.resync, await as(ADMIN, { app, form: {} }))).toMatchObject({
		status: 502,
		data: { error: "Synced every space, but HappyView didn't sync: lexicon upload failed: 401" }
	});
});

it('retries the audit copies still waiting, at once', async () => {
	t.net.failWrites(ROOT, 1);
	await signIn();

	expect(await callRoute(actions.retryAudit, await as(ADMIN, { form: {} }))).toEqual({
		success: 'Retried 1 audit entries'
	});
	expect((await loadRoute<PageServerData>(load, await as(ADMIN))).failed).toEqual([]);
});

it('plans the project migration', async () => {
	const { project } = await t.projects.publish({
		profile: { name: 'Saffron Social', kind: 'duo', genres: ['disco'], location: { city: 'Berlin', country: 'DE' } },
		managers: ['did:plc:dj'],
		status: 'active',
		createdBy: ADMIN
	});

	expect(await callRoute(actions.planMigration, await as(ADMIN, { form: {} }))).toMatchObject({
		plan: { totals: { acts: 1, djs: 0, blocked: 0 }, projects: [{ uri: project.uri, target: 'act' }] }
	});
});

it("says when HappyView doesn't answer the plan's reads", async () => {
	const down = new HappyViewError(503, 'HappyView /xrpc: fetch failed');
	const app = { ...systemApp(), projectMigration: { plan: () => Promise.reject(down) } };

	expect(await callRoute(actions.planMigration, await as(ADMIN, { app, form: {} }))).toMatchObject({
		status: 502,
		data: { error: "HappyView isn't answering, so the plan can't count press kits, shows and follows" }
	});
});

it("lets any other failure of the plan through, so it isn't blamed on HappyView", async () => {
	const broken = new Error('database is locked');
	const app = { ...systemApp(), projectMigration: { plan: () => Promise.reject(broken) } };

	await expect(callRoute(actions.planMigration, await as(ADMIN, { app, form: {} }))).rejects.toBe(broken);
});
