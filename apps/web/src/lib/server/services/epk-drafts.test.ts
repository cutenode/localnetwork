import { XRPCError } from '@atproto/api';
import type { JsonObject } from '@atproto/lex-json';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { COLLECTION, EPK_CONTENT_KEY } from '#lib/nsid.ts';
import { z } from 'zod';
import { recordCid } from '../atproto/repo-client';
import { ForbiddenError, NotFoundError, UserError } from '../core';
import { HappyViewClient } from '../happyview/client';
import { IndexedRepo } from '../happyview/indexed';
import { silentLogger } from '../logger';
import { ROOT } from '../testing/core';
import { EpkService } from './epk';
import { FakeRootClient } from '../testing/fakes';
import { ADMIN, CODJ, DJ, NO_SPACES, pressKitServices, STRANGER, values } from '../testing/press-kits';

describe('EpkDrafts', () => {
	let f: Awaited<ReturnType<typeof pressKitServices>>;

	beforeEach(async () => {
		f = await pressKitServices();
	});
	afterEach(async () => {
		vi.restoreAllMocks();
		await f.t.audit.idle();
		await f.t.db.destroy();
	});

	const save = async (did: string, rkey: string | undefined, overrides: Parameters<typeof values>[0] = {}) =>
		f.t.epks.drafts.save(await f.t.viewer(did), f.project, {
			rkey,
			cid: await f.opened(rkey),
			values: values(overrides)
		});

	const publish = async (did: string, rkey: string, overwrite?: Parameters<typeof f.t.epks.drafts.publish>[3]) =>
		f.t.epks.drafts.publish(await f.t.viewer(did), f.project, rkey, overwrite);

	const published = (rkey: string) => f.t.net.records.get(`${ROOT}|${COLLECTION.epk}|${rkey}`)!;

	const content = (space: string) =>
		f.t.net.spaceRecords.get(`${space}|${ROOT}|${COLLECTION.epkContent}|${EPK_CONTENT_KEY}`)!.value;

	it("keeps each manager's draft in their repo in the backstage, holding only the fields it changes", async () => {
		const epk = await f.publish(DJ);
		const backstage = f.t.epks.drafts.space(f.project);

		const draft = await save(CODJ, epk.rkey, { title: 'Clubs 2027', content: { ...values().content, techRider: '' } });

		expect(draft).toMatchObject({
			rkey: epk.rkey,
			author: { did: CODJ },
			repo: CODJ,
			base: { uri: epk.uri, cid: epk.cid },
			fields: ['title', 'content.techRider'],
			status: 'current'
		});
		expect(f.t.net.spaceRecords.get(`${backstage}|${CODJ}|${COLLECTION.epkDraft}|${epk.rkey}`)?.value).toMatchObject({
			changes: { title: 'Clubs 2027', content: { techRider: '' } },
			previous: { title: 'Clubs', content: { techRider: '2x CDJ-3000' } }
		});
		expect(Object.fromEntries(f.t.net.spaces.get(backstage)!.members)).toEqual({
			[DJ]: { read: true, write: true },
			[CODJ]: { read: true, write: true }
		});
		expect(f.t.net.spaces.get(backstage)?.writePolicy.$type).toBe('com.atproto.simplespace.defs#memberListPolicy');

		// Changing it back leaves nothing to publish, so the draft goes.
		expect(await save(CODJ, epk.rkey)).toBeNull();
		expect(f.t.net.spaceRecordsIn(backstage, CODJ)).toEqual([]);
		expect((await f.t.epks.dashboard(f.project)).kits[0].drafts).toEqual([]);
	});

	it("applies only a draft's changes onto the published press kit, so a co-manager's publish in between stays", async () => {
		const epk = await f.publish(DJ);

		await save(DJ, epk.rkey, { title: 'Clubs 2027' });
		await save(CODJ, epk.rkey, { content: { ...values().content, bio: 'A newer long bio' } });
		expect((await f.t.epks.dashboard(f.project)).kits[0].drafts.map((draft) => draft.status)).toEqual([
			'current',
			'current'
		]);

		expect((await publish(CODJ, epk.rkey)).kind).toBe('published');
		// DJ's draft started from the version before, so it reads as behind until it's published too.
		const [behind] = (await f.t.epks.dashboard(f.project)).kits[0].drafts;
		expect(behind).toMatchObject({ author: { did: DJ }, status: 'behind' });

		const result = await publish(DJ, epk.rkey);

		expect(result).toMatchObject({ kind: 'published', epk: { rkey: epk.rkey, record: { title: 'Clubs 2027' } } });
		expect(content(epk.space)).toMatchObject({ bio: 'A newer long bio', techRider: '2x CDJ-3000' });
		expect(published(epk.rkey).value).toMatchObject({ createdAt: epk.record.createdAt, headline: epk.record.headline });
		expect(f.t.net.spaceRecordsIn(f.t.epks.drafts.space(f.project))).toEqual([]);
		expect((await f.t.epks.dashboard(f.project)).kits[0].drafts).toEqual([]);
	});

	it('stops at a field someone else published since the draft changed it, and overwrites it only when told to', async () => {
		const epk = await f.publish(DJ);

		await save(DJ, epk.rkey, { headline: "DJ's headline", bio: 'Same new bio' });
		await save(CODJ, epk.rkey, { headline: "CODJ's headline", bio: 'Same new bio' });
		await publish(CODJ, epk.rkey);
		const after = published(epk.rkey).cid;

		const result = await publish(DJ, epk.rkey);

		// The bio both published the same way isn't a conflict.
		expect(result).toEqual({
			kind: 'conflict',
			conflicts: [
				{
					field: 'headline',
					published: "CODJ's headline",
					previous: 'Rolling techno for late rooms',
					draft: "DJ's headline"
				}
			]
		});
		expect(published(epk.rkey).cid).toBe(after);

		expect((await publish(DJ, epk.rkey, { overwrite: ['headline'] })).kind).toBe('published');
		expect(published(epk.rkey).value.headline).toBe("DJ's headline");
	});

	it('keeps the published value from when a field first changed, across saves of the same draft', async () => {
		const epk = await f.publish(DJ);

		await save(DJ, epk.rkey, { headline: 'First try' });
		await save(CODJ, epk.rkey, { headline: "CODJ's headline" });
		await publish(CODJ, epk.rkey);
		await save(DJ, epk.rkey, { headline: 'Second try' });

		expect((await publish(DJ, epk.rkey)).kind).toBe('conflict');
	});

	it("copies a draft's new photos into the root's repo on publish, and published ones a draft keeps into its writer's", async () => {
		const wide = await f.photo(DJ, 'wide');
		const secret = await f.photo(DJ, 'secret');
		const epk = await f.publish(DJ, { photos: [wide], content: { ...values().content, photos: [secret] } });
		const holders = (photo: typeof wide) => f.t.net.blobHolders.get(photo.image.ref.$link);

		expect(holders(wide)).toEqual(new Set([DJ, ROOT]));
		expect(holders(secret)).toEqual(new Set([DJ, ROOT]));

		// CODJ never uploaded these, so their PDS takes the draft only once they hold copies.
		const square = await f.photo(CODJ, 'square');
		await save(CODJ, epk.rkey, { photos: [square, wide], content: { ...values().content, photos: [secret] } });
		expect(holders(wide)).toEqual(new Set([DJ, ROOT, CODJ]));
		expect(holders(secret)).toEqual(new Set([DJ, ROOT]));
		// Clearing the private photos names them in what the draft replaces, so CODJ holds those too.
		await save(CODJ, epk.rkey, { photos: [square, wide], content: { ...values().content, photos: [] } });
		expect(holders(secret)).toEqual(new Set([DJ, ROOT, CODJ]));

		await publish(CODJ, epk.rkey);
		expect(holders(square)).toEqual(new Set([CODJ, ROOT]));
		expect(published(epk.rkey).value.photos).toMatchObject([{ alt: 'square' }, { alt: 'wide' }]);
		expect(content(epk.space).photos).toBeUndefined();
	});

	it("publishes even when the manager's session lapsed before their draft could be deleted", async () => {
		const epk = await f.publish(DJ);
		await save(DJ, epk.rkey, { title: 'Clubs 2027' });
		f.t.capable.delete(DJ);

		expect((await publish(DJ, epk.rkey)).kind).toBe('published');
		expect(published(epk.rkey).value.title).toBe('Clubs 2027');
		// The draft left behind holds nothing the published press kit doesn't.
		expect((await f.t.epks.dashboard(f.project)).kits[0].drafts.map((draft) => draft.status)).toEqual(['behind']);
	});

	it('makes a new press kit from a draft, under a key the root chooses', async () => {
		const draft = await save(CODJ, undefined, { title: 'Festivals' });

		expect(draft?.status).toBe('new');
		expect((await f.t.epks.dashboard(f.project)).drafts.map((entry) => entry.rkey)).toEqual([draft!.rkey]);
		expect(await save(CODJ, draft!.rkey, { title: 'Festivals 2027' })).toMatchObject({ rkey: draft!.rkey });

		const result = await publish(CODJ, draft!.rkey);

		if (result.kind !== 'published') throw new Error('a new press kit has nothing to conflict with');
		expect(result.epk).toMatchObject({ record: { title: 'Festivals 2027' } });
		expect(result.epk.rkey).not.toBe(draft!.rkey);
		expect(f.t.net.records.has(`${ROOT}|${COLLECTION.epk}|${draft!.rkey}`)).toBe(false);
		expect((await f.t.epks.dashboard(f.project)).drafts).toEqual([]);
	});

	it("finishes a new press kit's publish under the key it started with, and keeps a leftover draft with it", async () => {
		const draft = await save(DJ, undefined, { title: 'Festivals' });
		f.t.capable.delete(DJ);

		// The draft can't be deleted without DJ's session, so it stays, standing for the press kit it became.
		const result = await publish(DJ, draft!.rkey);

		if (result.kind !== 'published') throw new Error('a new press kit has nothing to conflict with');
		const { kits } = await f.t.epks.dashboard(f.project);

		expect(kits.map((kit) => [kit.epk.rkey, kit.drafts.map((entry) => entry.status)])).toEqual([
			[result.epk.rkey, ['behind']]
		]);
		expect(await publish(DJ, result.epk.rkey)).toMatchObject({ kind: 'published', epk: { rkey: result.epk.rkey } });
		expect(await f.t.epks.forProject(f.project)).toHaveLength(1);
	});

	it("writes a draft on the behalf of a manager whose PDS can't, under a key of its own", async () => {
		const epk = await f.publish(DJ);
		await f.setManagers([DJ, CODJ, NO_SPACES]);
		const manager = await f.t.viewer(NO_SPACES);

		const photo = {
			image: await f.t.epks.drafts.uploadPhoto(manager, new Uint8Array([1, 2]), 'image/png'),
			alt: 'tiny'
		};

		const draft = await f.t.epks.drafts.save(manager, f.project, {
			rkey: epk.rkey,
			cid: epk.cid,
			values: values({ photos: [photo] })
		});

		expect(draft).toMatchObject({ rkey: epk.rkey, author: { did: NO_SPACES }, repo: ROOT });
		const [written] = f.t.net.spaceRecordsIn(f.t.epks.drafts.space(f.project), ROOT);
		expect(written.uri).not.toContain(epk.rkey);
		expect(written.value).toMatchObject({ onBehalfOf: NO_SPACES, base: { uri: epk.uri } });

		expect((await f.t.epks.drafts.publish(manager, f.project, epk.rkey)).kind).toBe('published');
		expect(published(epk.rkey).value.photos).toMatchObject([{ alt: 'tiny' }]);
		expect(f.t.net.spaceRecordsIn(f.t.epks.drafts.space(f.project), ROOT)).toEqual([]);
	});

	it("says when a draft's press kit was deleted, and lets its manager discard it", async () => {
		const epk = await f.publish(DJ);
		await save(DJ, epk.rkey, { title: 'Clubs 2027' });
		await f.t.epks.remove(await f.t.viewer(CODJ), f.kit(epk));

		expect((await f.t.epks.dashboard(f.project)).drafts.map((draft) => draft.status)).toEqual(['deleted']);
		await expect(publish(DJ, epk.rkey)).rejects.toThrow(/was deleted/);

		await f.t.epks.drafts.discard(await f.t.viewer(DJ), f.project, epk.rkey);
		expect((await f.t.epks.dashboard(f.project)).drafts).toEqual([]);
		await expect(publish(DJ, epk.rkey)).rejects.toBeInstanceOf(NotFoundError);
	});

	it("only lets the project's managers draft, and only the current ones' drafts count", async () => {
		const epk = await f.publish(DJ);

		await expect(save(STRANGER, epk.rkey)).rejects.toBeInstanceOf(ForbiddenError);
		await expect(save(DJ, '3mynzwtu72z2t', { title: 'Nope' })).rejects.toThrow(/does not exist/);
		await expect(save(DJ, (await f.legacy(DJ)).rkey, { title: 'Nope' })).rejects.toThrow(/moves to the project/);

		await save(CODJ, epk.rkey, { title: 'Clubs 2027' });
		await f.setManagers([DJ]);
		expect((await f.t.epks.dashboard(f.project)).kits.flatMap((kit) => kit.drafts)).toEqual([]);
	});

	it('asks for a name before publishing a new press kit, and for a retry when someone published at the same moment', async () => {
		const nameless = await save(DJ, undefined, { title: '', access: undefined });
		await expect(publish(DJ, nameless!.rkey)).rejects.toThrow(/Give the press kit a name/);

		const epk = await f.publish(DJ);
		await save(DJ, epk.rkey, { title: 'Clubs 2027' });
		vi.spyOn(FakeRootClient.prototype, 'putRecord').mockRejectedValueOnce(new XRPCError(400, 'InvalidSwap'));
		await expect(publish(DJ, epk.rkey)).rejects.toMatchObject({ status: 409 });
		expect((await publish(DJ, epk.rkey)).kind).toBe('published');
	});

	it('saves a draft only over the published version its form was opened on', async () => {
		const epk = await f.publish(DJ);
		const codj = await f.t.viewer(CODJ);
		const stale = new UserError('This press kit changed since you opened it. Reload and try again.', 409);

		// DJ publishes again while CODJ's form is still open on the first version.
		await f.publish(DJ, { headline: 'Warehouse techno' }, epk.rkey);
		await expect(
			f.t.epks.drafts.save(codj, f.project, { rkey: epk.rkey, cid: epk.cid, values: values({ title: 'Clubs 2027' }) })
		).rejects.toEqual(stale);
		await expect(f.t.epks.drafts.save(codj, f.project, { rkey: epk.rkey, values: values() })).rejects.toEqual(stale);
		expect(f.t.net.spaceRecordsIn(f.t.epks.drafts.space(f.project), CODJ)).toEqual([]);

		// Reloaded, it saves.
		expect(await save(CODJ, epk.rkey, { title: 'Clubs 2027', headline: 'Warehouse techno' })).toMatchObject({
			status: 'current',
			fields: ['title']
		});
	});

	it("has HappyView re-read the root's press kits when the PDS holds a version it doesn't serve yet", async () => {
		const backfilled: string[] = [];

		const admin = new HappyViewClient({
			url: 'http://happyview.test',
			apiKey: 'hv_test',
			fetch: async (url, init) => {
				const request = new Request(url, init);

				if (new URL(request.url).pathname !== '/admin/backfill') return f.t.happyView.fetch(request);
				const { collection, did } = z.object({ collection: z.string(), did: z.string() }).parse(await request.json());
				backfilled.push(`${collection} ${did}`);

				return Response.json({ id: 'job', status: 'running' });
			}
		});

		const epks = new EpkService(f.t.core, f.t.projects, new IndexedRepo(admin, silentLogger, 0), async (id) => id);

		const epk = await f.publish(DJ);
		const dj = await f.t.viewer(DJ);
		await epks.drafts.save(dj, f.project, { rkey: epk.rkey, cid: epk.cid, values: values({ title: 'Clubs 2027' }) });

		// Another app changes it in the root's repo, and HappyView hasn't heard yet.
		const key = `${ROOT}|${COLLECTION.epk}|${epk.rkey}`;
		const elsewhere = { ...published(epk.rkey).value, headline: 'Changed elsewhere' };
		f.t.net.records.set(key, { uri: epk.uri, cid: await recordCid(elsewhere), value: elsewhere });

		await expect(epks.drafts.publish(dj, f.project, epk.rkey)).rejects.toMatchObject({ status: 409 });
		expect(published(epk.rkey).value.headline).toBe('Changed elsewhere');
		expect(backfilled).toEqual([`${COLLECTION.epk} ${ROOT}`]);
	});

	it("won't publish a draft onto another project's press kit, whatever key it names", async () => {
		const other = await f.t.projectAdmin.create({
			actor: await f.t.viewer(ADMIN),
			profile: { name: 'Elsewhere', kind: 'solo', genres: ['house'], location: { city: 'Leeds', country: 'GB' } },
			managers: [STRANGER]
		});

		const theirs = await f.t.epks.drafts.save(await f.t.viewer(STRANGER), other, { values: values() });
		const result = await f.t.epks.drafts.publish(await f.t.viewer(STRANGER), other, theirs!.rkey);

		if (result.kind !== 'published') throw new Error('a new press kit has nothing to conflict with');
		const target = result.epk;
		const space = f.t.epks.drafts.space(f.project);
		const now = new Date().toISOString();

		// CODJ writes drafts under the other project's key straight into this project's backstage.
		const write = (rkey: string, value: JsonObject) =>
			f.t.epks.drafts.index({
				space,
				uri: `${space}/${CODJ}/${COLLECTION.epkDraft}/${rkey}`,
				repoDid: CODJ,
				rkey,
				cid: 'c',
				value
			});

		await write(target.rkey, {
			changes: { title: 'Taken over' },
			base: { uri: target.uri, cid: target.cid },
			createdAt: now,
			updatedAt: now
		});
		await expect(publish(CODJ, target.rkey)).rejects.toThrow(/does not exist/);

		// Without a base it's a new press kit, which gets a key of its own.
		await write(target.rkey, {
			changes: { title: 'Taken over', access: values().access! },
			createdAt: now,
			updatedAt: now
		});
		const made = await publish(CODJ, target.rkey);
		expect(made).toMatchObject({ kind: 'published', epk: { record: { project: f.project.uri, title: 'Taken over' } } });
		expect(published(target.rkey).value).toMatchObject({ project: other.uri, title: 'Clubs' });
	});

	it('never has the root write a draft under a key a manager picked, and lists a new draft apart from the press kits', async () => {
		const epk = await f.publish(DJ);
		const space = f.t.epks.drafts.space(f.project);
		await save(CODJ, epk.rkey, { title: 'Clubs 2027' });
		f.t.capable.delete(CODJ);

		// CODJ's own draft is keyed by the press kit; written on their behalf now, the root's gets a key of its own.
		await save(CODJ, epk.rkey, { title: 'Clubs 2028' });
		const [written] = f.t.net.spaceRecordsIn(space, ROOT);
		expect(written.uri).not.toContain(epk.rkey);
		expect(written.value).toMatchObject({ onBehalfOf: CODJ, changes: { title: 'Clubs 2028' } });

		// A draft with no base under a published press kit's key is still a new press kit's.
		const now = new Date().toISOString();
		await f.t.epks.drafts.index({
			space,
			uri: `${space}/${DJ}/${COLLECTION.epkDraft}/${epk.rkey}`,
			repoDid: DJ,
			rkey: epk.rkey,
			cid: 'c',
			value: { changes: { title: 'Fresh' }, createdAt: now, updatedAt: now }
		});
		const dashboard = await f.t.epks.dashboard(f.project);
		expect(dashboard.kits[0].drafts.map((draft) => draft.author.did)).not.toContain(DJ);
		expect(dashboard.drafts).toMatchObject([{ author: { did: DJ }, status: 'new' }]);
	});

	it("indexes drafts synced from a backstage, by the manager they're for", async () => {
		const epk = await f.publish(DJ);
		const space = f.t.epks.drafts.space(f.project);
		const now = new Date().toISOString();

		const draft = {
			changes: { title: 'Synced' },
			base: { uri: epk.uri, cid: epk.cid },
			createdAt: now,
			updatedAt: now
		};

		const index = (repoDid: string, rkey: string, value: JsonObject) =>
			f.t.epks.drafts.index({
				space,
				uri: `${space}/${repoDid}/${COLLECTION.epkDraft}/${rkey}`,
				repoDid,
				rkey,
				cid: 'c',
				value
			});

		await index(CODJ, epk.rkey, draft);
		await index(ROOT, '3mynzwtu72z2t', { ...draft, onBehalfOf: DJ });
		// The root's draft naming nobody, a manager's under another press kit's key, and one of another account's press kit.
		await index(ROOT, '3mynzwtu72z2u', draft);
		await index(CODJ, '3mynzwtu72z2v', draft);
		await index(CODJ, epk.rkey, { ...draft, base: { uri: `at://${DJ}/${COLLECTION.epk}/${epk.rkey}`, cid: 'c' } });

		const drafts = (await f.t.epks.dashboard(f.project)).kits[0].drafts;
		expect(drafts.map((entry) => `${entry.author.did} ${entry.repo} ${entry.fields.join()}`).sort()).toEqual([
			`${CODJ} ${CODJ} title`,
			`${DJ} ${ROOT} title`
		]);

		await f.t.epks.drafts.unindex(drafts[0].uri);
		expect((await f.t.epks.dashboard(f.project)).kits[0].drafts).toHaveLength(1);
	});
});
