import { setTimeout as sleep } from 'node:timers/promises';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { z } from 'zod';
import { BSKY_FOLLOW, BSKY_PROFILE, CALENDAR_EVENT, COLLECTION, DIRECTORY } from '#lib/nsid.ts';
import type { ProjectProfile, ProjectView, StrongRef } from '#lib/types.ts';
import { UserError } from '../core';
import { HappyViewClient } from '../happyview/client';
import { createTestServices } from '../testing/core';
import { ProjectService } from './projects';
import { SetService } from './sets';

const ADMIN = 'did:plc:admin';

const MOD = 'did:plc:mod';

const DJ = 'did:plc:dj';

const STRANGER = 'did:plc:stranger';

const profile = (mixes: ProjectProfile['mixes'] = []): ProjectProfile => ({
	name: 'Linked',
	kind: 'solo',
	genres: ['house'],
	location: { city: 'Berlin', country: 'DE' },
	mixes
});

describe('linked sets', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;
	let project: ProjectView;

	/** What locallibrary's `sets publish` writes, straight to the DJ's repo, reaching HappyView over Jetstream. */
	const publishSet = (did: string, rkey: string, title: string): Promise<StrongRef> =>
		t.net.client(did).putRecord({
			collection: COLLECTION.djSet,
			rkey,
			record: { $type: COLLECTION.djSet, title, date: '2026-09-01', tracks: [{}], createdAt: '2026-09-02T00:00:00Z' }
		});

	const mix = (set?: StrongRef) => ({ title: 'Warehouse', url: 'https://mixes.test/warehouse', ...(set && { set }) });

	beforeEach(async () => {
		t = await createTestServices({ admins: [ADMIN], capable: [ADMIN, MOD, DJ] });
		await t.staff.grant(await t.viewer(ADMIN), MOD, 'moderator');
		project = await t.projectAdmin.create({ actor: await t.viewer(ADMIN), profile: profile(), managers: [DJ] });
	});
	afterEach(async () => {
		await t.audit.idle();
		await t.db.destroy();
	});

	it("publishes a manager's set once a moderator approves, pinned to the version picked", async () => {
		const set = await publishSet(DJ, '3kset', 'Closing');
		expect(await t.projects.sets.published([DJ])).toEqual([
			{ ...set, title: 'Closing', date: '2026-09-01', trackCount: 1 }
		]);

		const dj = await t.viewer(DJ);

		const request = await t.requests.submit({
			actor: dj,
			kind: 'update',
			projectUri: project.uri,
			profile: profile([mix(set)])
		});

		await t.requests.decide({ actor: await t.viewer(MOD), uri: request.uri, outcome: 'approved' });

		const published = (await t.projects.bySlug(project.slug))!;
		expect(published.profile.mixes?.[0]?.set).toEqual(set);

		// Editing the set later doesn't move the link: the page says it changed instead.
		const edited = await publishSet(DJ, '3kset', 'Closing (extended)');
		expect((await t.projects.sets.byUri([set])).get(set.uri)?.cid).toBe(edited.cid);
		expect(edited.cid).not.toBe(set.cid);
	});

	it("refuses a set that isn't there, isn't a set, or isn't by a manager", async () => {
		const admin = await t.viewer(ADMIN);
		const theirs = await publishSet(STRANGER, '3kset', 'Not theirs');
		const link = (set: StrongRef) => t.projectAdmin.update({ actor: admin, project, profile: profile([mix(set)]) });

		await expect(link(theirs)).rejects.toThrow(/someone who doesn't manage this project/);
		await expect(link({ uri: `at://${DJ}/${COLLECTION.djSet}/gone`, cid: theirs.cid })).rejects.toThrow(
			/“Warehouse” isn't published anymore/
		);
		await expect(link({ uri: project.uri, cid: project.cid })).rejects.toThrow(/isn't a set/);
		expect(await t.projects.sets.published([STRANGER])).toEqual([]);
	});

	it('keeps a published link as it is, even once the set is gone', async () => {
		const admin = await t.viewer(ADMIN);
		const set = await publishSet(DJ, '3kset', 'Closing');
		const linked = await t.projectAdmin.update({ actor: admin, project, profile: profile([mix(set)]) });
		await t.net.onRecord({ did: DJ, collection: COLLECTION.djSet, rkey: '3kset', cid: '', record: null });

		const edited = await t.projectAdmin.update({
			actor: admin,
			project: linked,
			profile: { ...profile([mix(set)]), tagline: 'Still linked' }
		});

		expect(edited.profile.mixes?.[0]?.set).toEqual(set);
		expect(await t.projects.sets.linked([set])).toEqual(new Map());
	});

	it("says so when HappyView can't answer: the picker gets null, and publishing asks to try again", async () => {
		const down = new SetService(
			t.core,
			new HappyViewClient({ url: 'http://happyview.test', fetch: async () => new Response('down', { status: 503 }) })
		);

		const set = { uri: `at://${DJ}/${COLLECTION.djSet}/3kset`, cid: project.cid };

		expect(await down.published([DJ])).toBeNull();
		expect(await down.linked([set])).toBeNull();
		expect(await down.published([])).toEqual([]);
		await expect(down.assertLinkable(profile([mix(set)]), undefined, [DJ])).rejects.toEqual(
			new UserError("Couldn't check the linked sets. Try again in a minute.", 503)
		);
	});

	it("has HappyView re-read an account's sets at most every ten minutes, and again after a failure", async () => {
		const calls: string[] = [];
		let failing = true;

		const admin = new HappyViewClient({
			url: 'http://happyview.test',
			apiKey: 'hv_test',
			fetch: async (input) => {
				const { pathname } = new URL(input instanceof Request ? input.url : input);
				calls.push(pathname);

				if (pathname === '/admin/backfill')
					return failing ? new Response('no', { status: 500 }) : Response.json({ id: 'job', status: 'running' });

				if (pathname === '/admin/backfill/status') return Response.json([{ id: 'job', status: 'completed' }]);

				return Response.json({ sets: [] });
			}
		});

		const sets = new SetService(t.core, admin);
		const backfills = () => calls.filter((path) => path === '/admin/backfill').length;

		expect(await sets.published([DJ])).toEqual([]);
		expect(backfills()).toBe(1);
		failing = false;
		await sets.published([DJ]);
		expect(backfills()).toBe(2);
		await sets.published([DJ]);
		expect(backfills()).toBe(2);
		expect(calls).toContain('/admin/backfill/status');
	});

	it('lists sets only once a re-read already running for the account is done', async () => {
		const calls: string[] = [];

		const admin = new HappyViewClient({
			url: 'http://happyview.test',
			apiKey: 'hv_test',
			fetch: async (input) => {
				const { pathname } = new URL(input instanceof Request ? input.url : input);
				calls.push(pathname);

				if (pathname === '/admin/backfill') return Response.json({ id: 'job', status: 'running' });

				if (pathname === '/admin/backfill/status') return Response.json([{ id: 'job', status: 'completed' }]);

				return Response.json({ sets: [] });
			}
		});

		const sets = new SetService(t.core, admin);
		const running = sets.refresh([DJ]);

		await sets.published([DJ]);
		await running;
		expect(calls).toEqual(['/admin/backfill', '/admin/backfill/status', `/xrpc/${DIRECTORY.listSets}`]);
	});

	it("has HappyView read a project's new managers' sets, stats, press kits, calendar events, and Bluesky profiles and follows once it's published", async () => {
		const backfilled: string[] = [];

		// Reads go to the test's HappyView; only the backfills are watched.
		const admin = new HappyViewClient({
			url: 'http://happyview.test',
			apiKey: 'hv_test',
			fetch: async (input, init) => {
				const request = new Request(input, init);
				const { pathname } = new URL(request.url);

				if (pathname === '/admin/backfill') {
					const { collection, did } = z.object({ collection: z.string(), did: z.string() }).parse(await request.json());
					backfilled.push(`${collection} ${did}`);

					return Response.json({ id: 'job', status: 'running' });
				}

				if (pathname === '/admin/backfill/status') return Response.json([{ id: 'job', status: 'completed' }]);

				return t.happyView.fetch(request);
			}
		});

		await new ProjectService(t.core, admin).publish({
			existing: project,
			profile: project.profile,
			managers: [DJ, STRANGER],
			status: 'active',
			createdBy: ADMIN
		});
		await expect
			.poll(() => backfilled.toSorted())
			.toEqual(
				[BSKY_PROFILE, BSKY_FOLLOW, CALENDAR_EVENT, COLLECTION.djProfile, COLLECTION.djSet, COLLECTION.epk].map(
					(collection) => `${collection} ${STRANGER}`
				)
			);
	});

	it("doesn't have HappyView read new managers' records before it serves the project that lets them in", async () => {
		const backfilled: string[] = [];

		const admin = new HappyViewClient({
			url: 'http://happyview.test',
			apiKey: 'hv_test',
			fetch: async (input, init) => {
				const request = new Request(input, init);
				const { pathname } = new URL(request.url);

				if (pathname === '/admin/backfill') backfilled.push('backfill');

				// HappyView is behind: it never serves the new version.
				if (pathname === `/xrpc/${DIRECTORY.getProject}`) return Response.json({});

				return t.happyView.fetch(request);
			}
		});

		await new ProjectService(t.core, admin, 50).publish({
			existing: project,
			profile: project.profile,
			managers: [DJ, STRANGER],
			status: 'active',
			createdBy: ADMIN
		});
		await sleep(50);
		expect(backfilled).toEqual([]);
	});

	it("tries a picker's re-read again when HappyView says it failed", async () => {
		const calls: string[] = [];

		const admin = new HappyViewClient({
			url: 'http://happyview.test',
			apiKey: 'hv_test',
			fetch: async (input) => {
				const { pathname } = new URL(input instanceof Request ? input.url : input);
				calls.push(pathname);

				if (pathname === '/admin/backfill') return Response.json({ id: 'job', status: 'running' });

				if (pathname === '/admin/backfill/status') return Response.json([{ id: 'job', status: 'failed' }]);

				return Response.json({ sets: [] });
			}
		});

		const sets = new SetService(t.core, admin);
		await sets.published([DJ]);
		await sets.published([DJ]);
		expect(calls.filter((path) => path === '/admin/backfill')).toHaveLength(2);
	});
});
