import { XRPCError } from '@atproto/api';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { EpkView } from '#lib/epk.ts';
import { epkContentSchema } from '#lib/epk-schema.ts';
import { COLLECTION, EPK_CONTENT_KEY } from '#lib/nsid.ts';
import { validateRecord } from '../atproto/lexicons';
import { epkSpace } from '../atproto/spaces';
import { ROOT } from '../testing/core';
import { FakeRootClient } from '../testing/fakes';
import { ADMIN, CODJ, DJ, FRIEND, PROMOTER, preset, pressKitServices, STRANGER, values } from '../testing/press-kits';

describe('LegacyEpks', () => {
	let f: Awaited<ReturnType<typeof pressKitServices>>;
	let legacy: EpkView;

	/** The press kit's new space under the root. */
	const space = () => epkSpace(ROOT, legacy.rkey);

	const migrate = async (did = DJ) => f.t.epks.migrateLegacy(await f.t.viewer(did));

	beforeEach(async () => {
		f = await pressKitServices();
		const wide = await f.photo(DJ, 'wide');
		const secret = await f.photo(DJ, 'secret');

		// Promoters get in at once, everyone else asks.
		legacy = await f.legacy(DJ, {
			access: preset('promoters'),
			photos: [wide],
			content: { ...values().content, photos: [secret] }
		});

		const owner = f.t.net.client(DJ);
		const createdAt = '2026-09-01T00:00:00.000Z';
		await owner.putMember({ space: legacy.space, did: FRIEND, read: true, write: false });
		await owner.putSpaceRecord({
			space: legacy.space,
			collection: COLLECTION.epkGrant,
			rkey: FRIEND,
			record: validateRecord(COLLECTION.epkGrant, { subject: FRIEND, via: 'invited', createdAt })
		});

		const elsewhere = await f.t.projectAdmin.create({
			actor: await f.t.viewer(ADMIN),
			profile: { name: 'Elsewhere', kind: 'solo', genres: ['house'], location: { city: 'Leeds', country: 'GB' } },
			managers: [FRIEND]
		});

		const promoter = await f.t.viewer(PROMOTER);
		await f.t.bookings.start({ actor: promoter, projectUri: elsewhere.uri, show: { title: 'Night' }, message: 'Hi' });
		// The promoter's rule approves them at once, but only the maker's session can write that grant until it moves.
		await f.t.epks.request(promoter, f.kit(legacy), {});
		await f.t.epks.request(await f.t.viewer(STRANGER), f.kit(legacy), { message: 'Please' });
	});
	afterEach(async () => {
		vi.restoreAllMocks();
		await f.t.audit.idle();
		await f.t.db.destroy();
	});

	/** Everything a moved press kit should have, and nothing left of the old one. */
	const expectMoved = async () => {
		const moved = f.t.net.records.get(`${ROOT}|${COLLECTION.epk}|${legacy.rkey}`)?.value;
		expect(moved).toMatchObject({ title: 'Clubs', project: f.project.uri, createdAt: legacy.record.createdAt });

		const content = f.t.net.spaceRecords.get(`${space()}|${ROOT}|${COLLECTION.epkContent}|${EPK_CONTENT_KEY}`);
		expect(content?.value).toMatchObject({ bio: 'The long bio', photos: [{ alt: 'secret' }] });

		for (const photo of [...legacy.record.photos!, ...epkContentSchema.parse(content!.value).photos!])
			expect(f.t.net.blobHolders.get(photo.image.ref.$link)?.has(ROOT)).toBe(true);

		expect(Object.fromEntries(f.t.net.spaces.get(space())!.members)).toEqual({
			[DJ]: { read: true, write: false },
			[CODJ]: { read: true, write: false },
			[FRIEND]: { read: true, write: false },
			[PROMOTER]: { read: true, write: false }
		});

		const [epk] = await f.t.epks.forProject(f.project);
		const kit = f.kit(epk);
		expect(epk).toMatchObject({ did: ROOT, rkey: legacy.rkey, legacy: false });
		expect((await f.t.epks.grants(epk)).map((grant) => `${grant.subject.did} ${grant.via}`).sort()).toEqual([
			`${FRIEND} invited`,
			`${PROMOTER} auto`
		]);
		expect(await f.t.epks.accessState(kit, await f.t.viewer(PROMOTER))).toMatchObject({ kind: 'granted' });

		const [request] = await f.t.epks.requests(epk);
		expect(request).toMatchObject({ requester: { did: STRANGER }, message: 'Please', status: 'pending' });
		expect(
			f.t.net.spaceRecordsIn(space(), ROOT).filter((record) => record.uri.includes(COLLECTION.epkRequest))
		).toHaveLength(2);
		expect(request.uri).toContain(space());

		expect(f.t.net.records.has(`${DJ}|${COLLECTION.epk}|${legacy.rkey}`)).toBe(false);
		expect(f.t.net.spaces.has(legacy.space)).toBe(false);
		expect(await f.t.db.selectFrom('epkRequest').where('spaceUri', '=', legacy.space).selectAll().execute()).toEqual(
			[]
		);
	};

	it("moves a maker's press kit to the root with its key, photos, private part, grants and open requests", async () => {
		expect(await migrate()).toEqual({ migrated: [legacy.rkey], failed: [] });
		await expectMoved();

		// Done is done: nothing is left to move on the next visit.
		expect(await migrate()).toEqual({ migrated: [], failed: [] });
		expect(await f.t.epks.forProject(f.project)).toHaveLength(1);
	});

	it('finishes a move that failed before the public record, without writing anything twice', async () => {
		let failed = false;

		// The root's PDS turns down its writes to the new space once the first request has moved there.
		vi.spyOn(f.t.net, 'assertCanWrite').mockImplementation((did, written) => {
			if (failed || did !== ROOT || !written.startsWith(`at://${ROOT}/`)) return;
			failed = f.t.net.spaceRecordsIn(written, ROOT).some((record) => record.uri.includes(COLLECTION.epkRequest));

			if (failed) throw new XRPCError(502, 'UpstreamFailure');
		});

		expect(await migrate()).toEqual({ migrated: [], failed: [legacy.rkey] });
		expect(f.t.net.records.has(`${ROOT}|${COLLECTION.epk}|${legacy.rkey}`)).toBe(false);
		expect((await f.t.epks.forProject(f.project)).map((epk) => epk.did)).toEqual([DJ]);

		expect(await migrate()).toEqual({ migrated: [legacy.rkey], failed: [] });
		await expectMoved();
	});

	it('survives a manager of another project publishing a draft under its key, then moves as usual', async () => {
		const other = await f.t.projectAdmin.create({
			actor: await f.t.viewer(ADMIN),
			profile: { name: 'Elsewhere', kind: 'solo', genres: ['house'], location: { city: 'Leeds', country: 'GB' } },
			managers: [STRANGER]
		});

		const stranger = await f.t.viewer(STRANGER);
		await f.t.epks.drafts.save(stranger, other, { values: values({ title: 'Mine' }) });
		const backstage = f.t.epks.drafts.space(other);
		const now = new Date().toISOString();
		const record = { changes: { title: 'Squat', access: values().access! }, createdAt: now, updatedAt: now };

		// Legacy keys are public in their maker's repo, so STRANGER writes a draft under one straight into their backstage.
		await f.t.net.client(STRANGER).putSpaceRecord({
			space: backstage,
			collection: COLLECTION.epkDraft,
			rkey: legacy.rkey,
			record: validateRecord(COLLECTION.epkDraft, record)
		});
		await f.t.epks.drafts.index({
			space: backstage,
			uri: `${backstage}/${STRANGER}/${COLLECTION.epkDraft}/${legacy.rkey}`,
			repoDid: STRANGER,
			rkey: legacy.rkey,
			cid: 'c',
			value: record
		});

		const result = await f.t.epks.drafts.publish(stranger, other, legacy.rkey);

		expect(result).toMatchObject({ kind: 'published', epk: { record: { project: other.uri, title: 'Squat' } } });
		expect(result.kind === 'published' && result.epk.rkey).not.toBe(legacy.rkey);
		expect(f.t.net.records.has(`${ROOT}|${COLLECTION.epk}|${legacy.rkey}`)).toBe(false);
		expect((await f.t.epks.forProject(f.project)).map((epk) => epk.uri)).toEqual([legacy.uri]);

		expect(await migrate()).toEqual({ migrated: [legacy.rkey], failed: [] });
		await expectMoved();
	});

	it('leaves a legacy press kit where it is when the root holds its key for another project', async () => {
		const other = await f.t.projectAdmin.create({
			actor: await f.t.viewer(ADMIN),
			profile: { name: 'Elsewhere', kind: 'solo', genres: ['house'], location: { city: 'Leeds', country: 'GB' } },
			managers: [STRANGER]
		});

		// Without its photos, which only DJ's repo holds.
		const { photos: _photos, ...record } = f.t.net.records.get(`${DJ}|${COLLECTION.epk}|${legacy.rkey}`)!.value;
		await f.t.net
			.client(ROOT)
			.putRecord({ collection: COLLECTION.epk, rkey: legacy.rkey, record: { ...record, project: other.uri } });
		f.t.epks.forget();

		expect(await migrate()).toEqual({ migrated: [], failed: [] });
		expect(f.t.net.records.has(`${DJ}|${COLLECTION.epk}|${legacy.rkey}`)).toBe(true);
		expect(f.t.net.spaces.has(legacy.space)).toBe(true);
		expect(f.t.net.records.get(`${ROOT}|${COLLECTION.epk}|${legacy.rkey}`)?.value.project).toBe(other.uri);
	});

	it("leaves both legacy press kits where they are when a co-manager's has the same key", async () => {
		const { photos: _photos, ...record } = f.t.net.records.get(`${DJ}|${COLLECTION.epk}|${legacy.rkey}`)!.value;
		await f.t.net
			.client(CODJ)
			.putRecord({ collection: COLLECTION.epk, rkey: legacy.rkey, record: { ...record, title: 'Copy' } });
		f.t.epks.forget(CODJ);

		expect(await migrate(CODJ)).toEqual({ migrated: [], failed: [] });
		expect(await migrate()).toEqual({ migrated: [], failed: [] });
		expect(f.t.net.records.has(`${ROOT}|${COLLECTION.epk}|${legacy.rkey}`)).toBe(false);
		expect(f.t.net.records.has(`${DJ}|${COLLECTION.epk}|${legacy.rkey}`)).toBe(true);
	});

	it('carries one request per asker, even when two askers picked the same key', async () => {
		// ADMIN picks the same key for their request as an asker already in the space.
		const [first] = await f.t.epks.requests(legacy);
		const rkey = first.uri.split('/').at(-1)!;
		await f.t.epks.saveRequest({
			uri: `${legacy.space}/${ADMIN}/${COLLECTION.epkRequest}/${rkey}`,
			space: legacy.space,
			epk: legacy.uri,
			requester: ADMIN,
			role: null,
			message: 'Me too',
			rule: 'review',
			createdAt: new Date().toISOString()
		});

		expect(await migrate()).toEqual({ migrated: [legacy.rkey], failed: [] });
		const [moved] = await f.t.epks.forProject(f.project);
		expect((await f.t.epks.requests(moved)).map((request) => request.requester.did).sort()).toEqual(
			[ADMIN, STRANGER].sort()
		);
	});

	it('joins a move already running for the same account instead of racing it', async () => {
		const viewer = await f.t.viewer(DJ);
		const [first, second] = await Promise.all([f.t.epks.migrateLegacy(viewer), f.t.epks.migrateLegacy(viewer)]);

		expect(second).toBe(first);
		expect(first).toEqual({ migrated: [legacy.rkey], failed: [] });
		await expectMoved();
	});

	it("deletes the old copy on the next visit when the maker's PDS failed to the first time", async () => {
		// Only the maker deletes a record while moving it, once everything else is done.
		vi.spyOn(FakeRootClient.prototype, 'deleteRecord').mockRejectedValueOnce(new XRPCError(502, 'UpstreamFailure'));

		expect(await migrate()).toEqual({ migrated: [], failed: [legacy.rkey] });
		// The root holds the key, so the press kit has moved and the old record no longer counts.
		expect((await f.t.epks.forProject(f.project)).map((epk) => epk.did)).toEqual([ROOT]);

		expect(await migrate()).toEqual({ migrated: [legacy.rkey], failed: [] });
		await expectMoved();
	});

	it("leaves press kits that only their maker can move, of projects they manage, while they're signed in", async () => {
		expect(await migrate(CODJ)).toEqual({ migrated: [], failed: [] });

		f.t.capable.delete(DJ);
		expect(await migrate()).toEqual({ migrated: [], failed: [] });
		const lapsed = { ...(await f.t.viewer(DJ)), spacesCapable: true };
		expect(await f.t.epks.migrateLegacy(lapsed)).toEqual({ migrated: [], failed: [] });
		f.t.capable.add(DJ);

		await f.setManagers([CODJ]);
		expect(await migrate()).toEqual({ migrated: [], failed: [] });
		expect(f.t.net.records.has(`${DJ}|${COLLECTION.epk}|${legacy.rkey}`)).toBe(true);
	});
});
