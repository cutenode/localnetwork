import { TID } from '@atproto/common-web';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { type EpkAccess, type EpkView, MAX_EPKS_PER_PROJECT } from '#lib/epk.ts';
import { followSchema } from '#lib/epk-schema.ts';
import { BSKY_FOLLOW, COLLECTION, EPK_CONTENT_KEY, SPACE_TYPE } from '#lib/nsid.ts';
import { isValidDid, isValidRecordKey } from '@atproto/syntax';
import { RecordValidationError, TOO_DEEP } from '../atproto/lexicons';
import { memberSpacePolicy } from '../atproto/repo-client';
import { epkRequestSpace, epkSpace } from '../atproto/spaces';
import { ensureRootSpaces } from '../bootstrap';
import { ForbiddenError, UserError } from '../core';
import { HappyViewClient, HappyViewError } from '../happyview/client';
import { IndexedRepo } from '../happyview/indexed';
import { silentLogger } from '../logger';
import { ROOT } from '../testing/core';
import { EpkService, grantKey } from './epk';
import {
	ADMIN,
	CODJ,
	DJ,
	FRIEND,
	NO_SPACES,
	PROMOTER,
	preset,
	pressKitServices,
	STRANGER,
	values
} from '../testing/press-kits';

describe('EpkService', () => {
	let f: Awaited<ReturnType<typeof pressKitServices>>;

	beforeEach(async () => {
		f = await pressKitServices();
	});
	afterEach(async () => {
		await f.t.audit.idle();
		await f.t.db.destroy();
	});

	/** A promoter with a booking thread for `uri`. */
	const book = async (did: string, uri: string) => {
		const actor = await f.t.viewer(did);
		await f.t.bookings.start({ actor, projectUri: uri, show: { title: 'Night' }, message: 'Hi' });
	};

	const requestSpace = () => epkRequestSpace(ROOT, f.project.rkey);

	/** The grant the root wrote for `did` to see `epk`, in the project's request space. */
	const grantOf = (epk: EpkView, did: string) =>
		f.t.net.spaceRecords.get(`${requestSpace()}|${ROOT}|${COLLECTION.epkGrant}|${grantKey(epk.rkey, did)}`)?.value;

	it("publishes press kits for the project in the root's repo, with the rest in a root-owned space each", async () => {
		const clubs = await f.publish(DJ);
		const festivals = await f.publish(CODJ, { title: 'Festivals' });

		expect((await f.t.epks.forProject(f.project)).map((epk) => epk.record.title)).toEqual(['Festivals', 'Clubs']);
		expect([clubs.did, clubs.legacy, clubs.space]).toEqual([
			ROOT,
			false,
			`at://${ROOT}/space/${SPACE_TYPE.epk}/${clubs.rkey}`
		]);
		expect(clubs.space).not.toBe(festivals.space);

		const published = f.t.net.records.get(`${ROOT}|${COLLECTION.epk}|${clubs.rkey}`)!.value;
		expect(published).toMatchObject({ project: f.project.uri, mixes: [{ title: 'Public mix' }] });
		expect(JSON.stringify(published)).not.toContain('book@dj.test');

		const content = f.t.net.spaceRecords.get(`${clubs.space}|${ROOT}|${COLLECTION.epkContent}|${EPK_CONTENT_KEY}`);
		expect(content?.value).toMatchObject({ bio: 'The long bio', contact: { email: 'book@dj.test' } });
		expect(Object.fromEntries(f.t.net.spaces.get(clubs.space)!.members)).toEqual({
			[DJ]: { read: true, write: false },
			[CODJ]: { read: true, write: false }
		});
		// Only the root writes there: requests go into the project's request space.
		expect(f.t.net.spaces.get(clubs.space)?.policy).toEqual(memberSpacePolicy());
		expect(f.t.net.spaces.has(requestSpace())).toBe(false);
	});

	it('finds a press kit by project and key, and from an old link by the repo that held it', async () => {
		const epk = await f.publish(DJ);

		expect((await f.t.epks.get(f.project, epk.rkey))?.epk.uri).toBe(epk.uri);
		expect(await f.t.epks.get(f.project, 'not-a-key')).toBeNull();
		expect((await f.t.epks.locate(DJ, epk.rkey))?.project.uri).toBe(f.project.uri);
		expect((await f.t.epks.locate(ROOT, epk.rkey))?.epk.uri).toBe(epk.uri);
		expect(await f.t.epks.locate(ROOT, 'not-a-key')).toBeNull();

		const other = await f.t.projectAdmin.create({
			actor: await f.t.viewer(ADMIN),
			profile: { name: 'Elsewhere', kind: 'solo', genres: ['house'], location: { city: 'Leeds', country: 'GB' } }
		});

		expect(await f.t.epks.get(other, epk.rkey)).toBeNull();
	});

	it('lists a legacy press kit until the root holds its key, and only while its maker manages the project', async () => {
		const legacy = await f.legacy(DJ, { title: 'Old clubs' });
		const shared = await f.publish(CODJ, { title: 'Festivals' });

		expect(legacy).toMatchObject({ did: DJ, legacy: true });
		expect((await f.t.epks.forProject(f.project)).map((epk) => epk.record.title).sort()).toEqual([
			'Festivals',
			'Old clubs'
		]);
		expect((await f.t.epks.locate(DJ, legacy.rkey))?.epk.uri).toBe(legacy.uri);

		// The root holding the same key means it moved, so the root's copy wins wherever both are found.
		const root = f.t.net.client(ROOT);
		const moved = f.t.net.records.get(`${DJ}|${COLLECTION.epk}|${legacy.rkey}`)!.value;
		await root.putRecord({ collection: COLLECTION.epk, rkey: legacy.rkey, record: { ...moved, title: 'Moved clubs' } });
		f.t.epks.forget();

		const kits = await f.t.epks.forProject(f.project);
		expect(kits.map((epk) => `${epk.record.title} ${epk.did}`).sort()).toEqual([
			`Festivals ${ROOT}`,
			`Moved clubs ${ROOT}`
		]);
		expect((await f.t.epks.get(f.project, legacy.rkey))?.epk.did).toBe(ROOT);
		expect((await f.t.epks.locate(DJ, legacy.rkey))?.epk.did).toBe(ROOT);

		const kept = await f.legacy(DJ, { title: 'Kept' });
		await f.setManagers([CODJ]);
		f.t.epks.forget(DJ);
		expect((await f.t.epks.forProject(f.project)).map((epk) => epk.uri)).not.toContain(kept.uri);
		expect(await f.t.epks.locate(DJ, kept.rkey)).toBeNull();
		expect((await f.t.epks.forProject(f.project)).map((epk) => epk.uri)).toContain(shared.uri);
	});

	it('keeps listing a legacy press kit whose key the root holds for another project', async () => {
		const legacy = await f.legacy(DJ, { title: 'Old clubs' });

		const other = await f.t.projectAdmin.create({
			actor: await f.t.viewer(ADMIN),
			profile: { name: 'Elsewhere', kind: 'solo', genres: ['house'], location: { city: 'Leeds', country: 'GB' } },
			managers: [STRANGER]
		});

		// Some other project's press kit under the same key, as a clash or a squat.
		const record = f.t.net.records.get(`${DJ}|${COLLECTION.epk}|${legacy.rkey}`)!.value;
		await f.t.net.client(ROOT).putRecord({
			collection: COLLECTION.epk,
			rkey: legacy.rkey,
			record: { ...record, project: other.uri, title: 'Squat' }
		});
		f.t.epks.forget();

		expect((await f.t.epks.forProject(f.project)).map((epk) => epk.uri)).toEqual([legacy.uri]);
		expect((await f.t.epks.forProject(other)).map((epk) => epk.record.title)).toEqual(['Squat']);
		expect((await f.t.epks.get(f.project, legacy.rkey))?.epk.uri).toBe(legacy.uri);
		expect((await f.t.epks.get(other, legacy.rkey))?.epk.did).toBe(ROOT);
		expect((await f.t.epks.locate(DJ, legacy.rkey))?.epk.uri).toBe(legacy.uri);
		expect((await f.t.epks.locate(ROOT, legacy.rkey))?.project.uri).toBe(other.uri);
	});

	it('caps a project at its number of press kits, counting the legacy ones', async () => {
		await f.legacy(DJ, { title: 'Legacy' });

		for (let n = 1; n < MAX_EPKS_PER_PROJECT; n++) await f.publish(n % 2 ? DJ : CODJ, { title: `Kit ${n}` });

		await expect(f.publish(CODJ, { title: 'One too many' })).rejects.toThrow(/at most 20 press kits/);
		expect(await f.t.epks.forProject(f.project)).toHaveLength(MAX_EPKS_PER_PROJECT);
	});

	it('keeps the private part from people until any manager approves them, and the root writes the grant', async () => {
		const epk = await f.publish(DJ);
		const kit = f.kit(epk);
		const stranger = await f.t.viewer(STRANGER);

		expect(await f.t.epks.accessState(kit, null)).toEqual({ kind: 'signedOut', rule: 'review' });
		expect(await f.t.epks.accessState(kit, stranger)).toEqual({ kind: 'canAsk', rule: 'review' });
		expect(await f.t.epks.accessState(kit, await f.t.viewer(CODJ))).toEqual({ kind: 'manager' });

		expect((await f.t.epks.request(stranger, kit, { role: 'press', message: 'For a feature' })).kind).toBe('pending');
		const [request] = await f.t.epks.requests(epk);
		expect(request).toMatchObject({ role: 'press', message: 'For a feature', status: 'pending', rule: 'review' });
		expect(f.t.net.spaceRecordsIn(requestSpace(), STRANGER)).toHaveLength(1);
		expect(f.t.net.spaceRecordsIn(epk.space, STRANGER)).toEqual([]);

		await expect(f.t.epks.decide(stranger, kit, request.uri, 'approve')).rejects.toBeInstanceOf(ForbiddenError);
		// The manager who didn't make it answers just the same.
		await f.t.epks.decide(await f.t.viewer(CODJ), kit, request.uri, 'approve');

		expect((await f.t.epks.accessState(kit, stranger)).kind).toBe('granted');
		expect(grantOf(epk, STRANGER)).toMatchObject({
			subject: STRANGER,
			epk: epk.uri,
			via: 'approved',
			request: request.uri
		});
		expect(f.t.net.spaces.get(epk.space)?.members.get(STRANGER)).toEqual({ read: true, write: false });
		// Those let in read the press kit's space, which holds the private part and nothing about anyone else.
		expect(f.t.net.spaceRecordsIn(epk.space).map((record) => record.uri.split('/').at(-2))).toEqual([
			COLLECTION.epkContent
		]);
		// Being let in doesn't let them read anyone's request, their own included: only the managers read those.
		expect(Object.fromEntries(f.t.net.spaces.get(requestSpace())!.members)).toEqual({
			[DJ]: { read: true, write: false },
			[CODJ]: { read: true, write: false }
		});
		expect(f.t.net.spaces.get(requestSpace())?.policy).toEqual(memberSpacePolicy({ publicWrite: true }));
		expect((await f.t.epks.content(epk))?.techRider).toBe('2x CDJ-3000');
		expect(await f.t.epks.request(stranger, kit, {})).toMatchObject({ kind: 'granted' });
	});

	it('reads the private part as none when the space host serves it too deep to read', async () => {
		const epk = await f.publish(DJ);
		// What the real reader throws for a record nested deeper than 32 levels.
		f.t.core.readSpaceRecord = async () => {
			throw new RecordValidationError(COLLECTION.epkContent, TOO_DEEP);
		};

		expect(await f.t.epks.content(epk)).toBeNull();
	});

	it('approves at once when the rules say so', async () => {
		const epk = await f.publish(DJ, { access: preset('open') });

		expect((await f.t.epks.request(await f.t.viewer(STRANGER), f.kit(epk), {})).kind).toBe('granted');
		expect(grantOf(epk, STRANGER)?.via).toBe('auto');
		expect(await f.t.epks.requests(epk)).toEqual([]);
	});

	it("leaves an automatic approval of a legacy press kit waiting, since only its maker's space holds it", async () => {
		const epk = await f.legacy(DJ, { access: preset('open') });
		const kit = f.kit(epk);

		expect(await f.t.epks.request(await f.t.viewer(STRANGER), kit, {})).toMatchObject({
			kind: 'pending',
			approved: true
		});
		await expect(f.t.epks.invite(await f.t.viewer(DJ), kit, FRIEND)).rejects.toThrow(/moves to the project/);
		await expect(f.t.epks.remove(await f.t.viewer(CODJ), kit)).rejects.toThrow(/moves to the project/);
	});

	/** A Bluesky follow of `subject` in `did`'s repo, which Jetstream brings HappyView. */
	const follow = (did: string, subject: string) =>
		f.t.net.client(did).createRecord({
			collection: BSKY_FOLLOW,
			record: { $type: BSKY_FOLLOW, subject, createdAt: '2026-01-01T00:00:00Z' }
		});

	/** Whom HappyView keeps `did` following. */
	const followed = async (did: string) =>
		(await f.t.indexedRepo.listAll(did, BSKY_FOLLOW))
			.map(({ value }) => followSchema.parse(value).subject)
			.toSorted((a, b) => a.localeCompare(b));

	it("reads each audience from bookings and from any manager's follows", async () => {
		const access: EpkAccess = { bookedWith: 'auto', following: 'auto', promoters: 'review', anyone: 'none' };
		// CODJ follows them, not DJ who made the press kit, from before any press kit read follows, so HappyView
		// turned it away then.
		await follow(CODJ, NO_SPACES);
		expect(await followed(CODJ)).toEqual([]);
		const kit = f.kit(await f.publish(DJ, { access }));
		// The project's first press kit that reads follows has HappyView re-read its managers'.
		await vi.waitFor(async () => expect(await followed(CODJ)).toEqual([NO_SPACES]));

		const other = await f.t.projectAdmin.create({
			actor: await f.t.viewer(ADMIN),
			profile: { name: 'Elsewhere', kind: 'solo', genres: ['house'], location: { city: 'Leeds', country: 'GB' } },
			managers: [STRANGER]
		});

		await book(PROMOTER, other.uri);
		await book(FRIEND, f.project.uri);

		expect(await f.t.epks.ruleForViewer(kit, FRIEND)).toBe('auto');
		expect(await f.t.epks.ruleForViewer(kit, NO_SPACES)).toBe('auto');
		expect(await f.t.epks.ruleForViewer(kit, PROMOTER)).toBe('review');
		expect(await f.t.epks.accessState(kit, await f.t.viewer(ADMIN))).toEqual({ kind: 'closed' });
		await expect(f.t.epks.request(await f.t.viewer(ADMIN), kit, {})).rejects.toBeInstanceOf(UserError);
	});

	it("keeps only the follows of managers whose project's press kits read them, and re-reads a new manager's", async () => {
		await f.publish(DJ, { access: preset('review') });
		await follow(DJ, FRIEND);
		expect(await followed(DJ)).toEqual([]);

		await f.publish(CODJ, { access: { ...preset('review'), following: 'auto' } });
		await vi.waitFor(async () => expect(await followed(DJ)).toEqual([FRIEND]));
		await follow(CODJ, STRANGER);
		expect(await followed(CODJ)).toEqual([STRANGER]);
		await follow(NO_SPACES, FRIEND);
		expect(await followed(NO_SPACES)).toEqual([]);
		expect((await f.t.epks.followReaders()).toSorted()).toEqual([CODJ, DJ].toSorted());

		// Someone who comes to manage the project has their earlier follows re-read.
		await f.t.projectAdmin.setManagers({
			actor: await f.t.viewer(ADMIN),
			project: f.project,
			managers: [DJ, CODJ, NO_SPACES]
		});
		await vi.waitFor(async () => expect(await followed(NO_SPACES)).toEqual([FRIEND]));
	});

	it('takes no requests when nobody can ask, but any manager can add and remove people', async () => {
		const epk = await f.publish(DJ, { access: preset('closed') });
		const kit = f.kit(epk);

		expect(await f.t.epks.accessState(kit, null)).toEqual({ kind: 'closed' });
		await f.t.epks.invite(await f.t.viewer(CODJ), kit, FRIEND);
		expect(grantOf(epk, FRIEND)?.via).toBe('invited');
		expect((await f.t.epks.grants(epk)).map((grant) => grant.via)).toEqual(['invited']);
		await expect(f.t.epks.invite(await f.t.viewer(CODJ), kit, 'nobody.test')).rejects.toThrow(/Couldn't find @nobody/);
		await expect(f.t.epks.invite(await f.t.viewer(CODJ), kit, DJ)).rejects.toThrow(/already see/);

		await f.t.epks.revoke(await f.t.viewer(DJ), kit, FRIEND);
		expect(grantOf(epk, FRIEND)).toBeUndefined();
		expect(f.t.net.spaces.get(epk.space)?.members.has(FRIEND)).toBe(false);
		expect(await f.t.epks.accessState(kit, await f.t.viewer(FRIEND))).toEqual({ kind: 'closed' });
	});

	it('tells someone who was declined, and lets nobody ask twice', async () => {
		const kit = f.kit(await f.publish(DJ));
		const stranger = await f.t.viewer(STRANGER);

		await f.t.epks.request(stranger, kit, {});
		await expect(f.t.epks.request(stranger, kit, {})).rejects.toThrow(/already asked/);
		const [request] = await f.t.epks.requests(kit.epk);
		await expect(f.t.epks.decide(await f.t.viewer(DJ), kit, 'at://missing', 'decline')).rejects.toThrow(
			/does not exist/
		);
		await f.t.epks.decide(await f.t.viewer(CODJ), kit, request.uri, 'decline');

		expect(await f.t.epks.accessState(kit, stranger)).toEqual({ kind: 'declined' });
		await expect(f.t.epks.request(stranger, kit, {})).rejects.toThrow(/didn't share/);
		await expect(f.t.epks.decide(await f.t.viewer(DJ), kit, request.uri, 'approve')).rejects.toThrow(
			/already been answered/
		);
	});

	it("writes the request for someone whose PDS can't, on their behalf", async () => {
		const kit = f.kit(await f.publish(DJ));

		await f.t.epks.request(await f.t.viewer(NO_SPACES), kit, {});

		const written = f.t.net
			.spaceRecordsIn(requestSpace(), ROOT)
			.find((record) => record.uri.includes(COLLECTION.epkRequest));

		expect(written?.value).toMatchObject({ epk: kit.epk.uri, onBehalfOf: NO_SPACES });
		expect((await f.t.epks.requests(kit.epk))[0].requester.did).toBe(NO_SPACES);
	});

	it("lets any manager delete a press kit with its space, and everyone's access with it", async () => {
		const epk = await f.publish(DJ, { access: preset('open') });
		const other = await f.publish(DJ, { title: 'Festivals' });
		const kit = f.kit(epk);
		await f.t.epks.request(await f.t.viewer(STRANGER), kit, {});
		await f.t.epks.request(await f.t.viewer(NO_SPACES), kit, {});
		await f.t.epks.request(await f.t.viewer(NO_SPACES), f.kit(other), {});
		// A grant not moved out of the press kit's space yet goes with the space.
		await f.grantedBefore(epk, { subject: FRIEND, via: 'invited', createdAt: new Date().toISOString() });

		await expect(f.t.epks.remove(await f.t.viewer(STRANGER), kit)).rejects.toBeInstanceOf(ForbiddenError);
		await f.t.epks.remove(await f.t.viewer(CODJ), kit);

		expect(f.t.net.spaces.has(epk.space)).toBe(false);
		expect(f.t.net.records.has(`${ROOT}|${COLLECTION.epk}|${epk.rkey}`)).toBe(false);
		expect((await f.t.epks.forProject(f.project)).map((kept) => kept.uri)).toEqual([other.uri]);
		expect(await f.t.epks.grants(epk)).toEqual([]);
		// The requests the root wrote for it and its grants go too; the asker's own request is theirs to delete, and the
		// other press kit's stays.
		expect(f.t.net.spaceRecordsIn(requestSpace(), ROOT).map((record) => record.value.epk)).toEqual([other.uri]);
		expect(f.t.net.spaceRecordsIn(requestSpace(), STRANGER)).toHaveLength(1);
		expect((await f.t.epks.requests(other)).map((request) => request.requester.did)).toEqual([NO_SPACES]);
	});

	it('fails a change or an access decision while HappyView is down, rather than taking a press kit for missing', async () => {
		const epk = await f.publish(DJ);
		const dj = await f.t.viewer(DJ);
		const draft = await f.t.epks.drafts.save(dj, f.project, { values: values({ title: 'Festivals' }) });
		const unanswered = async () => new Response('down', { status: 503 });

		const down = new EpkService(
			f.t.core,
			f.t.projects,
			new IndexedRepo(new HappyViewClient({ url: 'http://happyview.test', fetch: unanswered }), silentLogger, 0),
			async (id) => id
		);

		await expect(down.get(f.project, epk.rkey)).rejects.toBeInstanceOf(HappyViewError);
		await expect(down.locate(ROOT, epk.rkey)).rejects.toBeInstanceOf(HappyViewError);
		await expect(
			down.drafts.save(dj, f.project, { rkey: epk.rkey, cid: epk.cid, values: values({ title: 'Edited' }) })
		).rejects.toBeInstanceOf(HappyViewError);
		await expect(down.drafts.publish(dj, f.project, draft!.rkey)).rejects.toBeInstanceOf(HappyViewError);
		expect(f.t.net.records.get(`${ROOT}|${COLLECTION.epk}|${epk.rkey}`)?.value.title).toBe('Clubs');
		// Pages that list press kits still render without it.
		expect(await down.forProject(f.project)).toEqual([]);
	});

	it('has the bootstrap watch the press kit, request spaces and backstages the root made', async () => {
		const epk = await f.publish(DJ);
		await f.t.epks.request(await f.t.viewer(STRANGER), f.kit(epk), {});

		expect(await ensureRootSpaces(f.t.core)).toEqual(
			expect.arrayContaining([epk.space, requestSpace(), f.t.epks.drafts.space(f.project)])
		);
	});

	it('indexes requests and grants synced from press kit spaces, by the rules of the press kit they name', async () => {
		const kit = f.kit(await f.publish(DJ));
		const createdAt = new Date().toISOString();
		const uri = (repo: string) => `${kit.epk.space}/${repo}/${COLLECTION.epkRequest}/3req`;

		await f.t.epks.indexRequest({
			space: kit.epk.space,
			uri: uri(STRANGER),
			repoDid: STRANGER,
			value: { epk: kit.epk.uri, createdAt }
		});
		// A manager's own request, and one the root wrote naming nobody, aren't anyone's.
		await f.t.epks.indexRequest({
			space: kit.epk.space,
			uri: uri(CODJ),
			repoDid: CODJ,
			value: { epk: kit.epk.uri, createdAt }
		});
		await f.t.epks.indexRequest({
			space: kit.epk.space,
			uri: uri(ROOT),
			repoDid: ROOT,
			value: { epk: kit.epk.uri, createdAt }
		});
		// Nor is one that isn't a request at all.
		await f.t.epks.indexRequest({
			space: kit.epk.space,
			uri: uri(FRIEND),
			repoDid: FRIEND,
			value: { epk: kit.epk.uri }
		});
		expect((await f.t.epks.requests(kit.epk)).map((request) => request.requester.did)).toEqual([STRANGER]);

		await f.t.epks.unindexRequest(uri(STRANGER));
		expect(await f.t.epks.requests(kit.epk)).toEqual([]);

		// In a project's request space, a request counts for the press kit it names, when that's one of the project's.
		const theirs = await f.t.projectAdmin.create({
			actor: await f.t.viewer(ADMIN),
			profile: { name: 'Squat', kind: 'solo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } }
		});

		const asked = (space: string, epk: string, rkey: string) =>
			f.t.epks.indexRequest({
				space,
				uri: `${space}/${FRIEND}/${COLLECTION.epkRequest}/${rkey}`,
				repoDid: FRIEND,
				value: { epk, createdAt }
			});

		await asked(epkRequestSpace(ROOT, theirs.rkey), kit.epk.uri, '3other');
		await asked(epkRequestSpace(STRANGER, f.project.rkey), kit.epk.uri, '3notroot');
		await asked(requestSpace(), `at://${ROOT}/${COLLECTION.project}/${f.project.rkey}`, '3notkit');
		expect(await f.t.epks.requests(kit.epk)).toEqual([]);

		await asked(requestSpace(), kit.epk.uri, '3mine');
		expect(await f.t.epks.requests(kit.epk)).toMatchObject([
			{ uri: `${requestSpace()}/${FRIEND}/${COLLECTION.epkRequest}/3mine`, requester: { did: FRIEND } }
		]);

		await f.t.epks.indexGrant({
			space: kit.epk.space,
			uri: `${kit.epk.space}/${ROOT}/${COLLECTION.epkGrant}/${STRANGER}`,
			repoDid: ROOT,
			value: { subject: STRANGER, via: 'approved', createdAt }
		});
		expect((await f.t.epks.accessState(kit, await f.t.viewer(STRANGER))).kind).toBe('granted');

		// A request synced from the request space by someone already let in is answered already.
		await f.t.epks.indexRequest({
			space: requestSpace(),
			uri: `${requestSpace()}/${STRANGER}/${COLLECTION.epkRequest}/3again`,
			repoDid: STRANGER,
			value: { epk: kit.epk.uri, createdAt }
		});

		const again = await f.t.db
			.selectFrom('epkRequest')
			.select('status')
			.where('uri', '=', `${requestSpace()}/${STRANGER}/${COLLECTION.epkRequest}/3again`)
			.executeTakeFirst();

		expect(again?.status).toBe('approved');
	});

	it('indexes grants from the request space only for the root press kit of its project they name', async () => {
		const kit = f.kit(await f.publish(DJ));
		const legacy = await f.legacy(DJ);
		const grant = { subject: STRANGER, epk: kit.epk.uri, via: 'approved', createdAt: new Date().toISOString() };

		const uri = (space: string, repo: string) =>
			`${space}/${repo}/${COLLECTION.epkGrant}/${grantKey(kit.epk.rkey, STRANGER)}`;

		const synced = (space: string, repoDid: string, value: Record<string, string>) =>
			f.t.epks.indexGrant({ space, uri: uri(space, repoDid), repoDid, value });

		const theirs = await f.t.projectAdmin.create({
			actor: await f.t.viewer(ADMIN),
			profile: { name: 'Squat', kind: 'solo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } }
		});

		// Anyone can write into the request space, but only the root grants; a grant names a press kit of the space's
		// project the root holds, and only the request space and press kits' spaces hold grants.
		await synced(requestSpace(), STRANGER, grant);
		await synced(requestSpace(), ROOT, { subject: STRANGER, via: 'approved', createdAt: grant.createdAt });
		await synced(epkRequestSpace(ROOT, theirs.rkey), ROOT, grant);
		await synced(requestSpace(), ROOT, { ...grant, epk: legacy.uri });
		await synced(f.t.epks.drafts.space(f.project), ROOT, grant);
		expect(await f.t.db.selectFrom('epkGrant').selectAll().execute()).toEqual([]);

		await synced(requestSpace(), ROOT, grant);
		expect(await f.t.db.selectFrom('epkGrant').select(['spaceUri', 'subjectDid', 'uri']).execute()).toEqual([
			{ spaceUri: kit.epk.space, subjectDid: STRANGER, uri: uri(requestSpace(), ROOT) }
		]);
		expect(await f.t.epks.accessState(kit, await f.t.viewer(STRANGER))).toMatchObject({ kind: 'granted' });

		await f.t.epks.unindexGrant(uri(requestSpace(), ROOT));
		expect(await f.t.epks.accessState(kit, await f.t.viewer(STRANGER))).toMatchObject({ kind: 'canAsk' });
	});

	it('keys a grant by press kit and subject, a record key even for a did:web with a port', async () => {
		const epk = await f.publish(DJ);
		const web = 'did:web:localhost%3A2583';
		const grant = { subject: web, via: 'invited', createdAt: new Date().toISOString() } as const;

		expect(grantKey(epk.rkey, DJ)).toBe(`${epk.rkey}:${DJ}`);
		expect(grantKey(epk.rkey, web)).toBe(`${epk.rkey}:did:web:localhost~3A2583`);
		expect([isValidRecordKey(web), isValidRecordKey(grantKey(epk.rkey, web))]).toEqual([false, true]);
		// No DID has a "~", so no other subject's key is the same.
		expect(isValidDid('did:web:localhost~3A2583')).toBe(false);

		await f.t.epks.writeGrant(f.project, epk, grant);
		await f.t.epks.writeGrant(f.project, epk, grant);
		expect(grantOf(epk, web)).toMatchObject({ subject: web, epk: epk.uri });
		expect(f.t.net.spaceRecordsIn(requestSpace(), ROOT)).toHaveLength(1);
	});

	it("takes away a grant not moved out of the press kit's space yet", async () => {
		const kit = f.kit(await f.publish(DJ, { access: preset('closed') }));
		await f.grantedBefore(kit.epk, { subject: FRIEND, via: 'invited', createdAt: new Date().toISOString() });
		expect(await f.t.epks.accessState(kit, await f.t.viewer(FRIEND))).toMatchObject({ kind: 'granted' });

		await f.t.epks.revoke(await f.t.viewer(CODJ), kit, FRIEND);
		expect(f.t.net.spaceRecordsIn(kit.epk.space).map((record) => record.uri.split('/').at(-2))).toEqual([
			COLLECTION.epkContent
		]);
		expect(f.t.net.spaces.get(kit.epk.space)?.members.has(FRIEND)).toBe(false);
		expect(await f.t.epks.accessState(kit, await f.t.viewer(FRIEND))).toEqual({ kind: 'closed' });
	});
});

describe('EpkService reads', () => {
	let f: Awaited<ReturnType<typeof pressKitServices>>;

	beforeEach(async () => {
		f = await pressKitServices();
	});
	afterEach(async () => {
		await f.t.audit.idle();
		await f.t.db.destroy();
	});

	/** The same services without a way to download blobs. */
	const blind = () =>
		new EpkService(
			{ ...f.t.core, fetchBlob: undefined, fetchSpaceBlob: undefined },
			f.t.projects,
			f.t.indexedRepo,
			async (id) => id
		);

	it('serves public photos to anyone and private ones only with the private part', async () => {
		const wide = await f.photo(DJ, 'wide');
		const secret = await f.photo(DJ, 'secret');
		const epk = await f.publish(DJ, { photos: [wide], content: { ...values().content, photos: [secret] } });
		const cid = (photo: typeof wide) => photo.image.ref.$link;
		const text = (found: { bytes: Uint8Array } | null) => found && new TextDecoder().decode(found.bytes);

		const shown = await f.t.epks.photo(epk, cid(wide), false);
		expect([text(shown), shown?.isPublic]).toEqual(['wide', true]);
		expect(await f.t.epks.photo(epk, cid(secret), false)).toBeNull();

		const hidden = await f.t.epks.photo(epk, cid(secret), true);
		expect([text(hidden), hidden?.isPublic]).toEqual(['secret', false]);
		expect(await f.t.epks.photo(epk, 'bafkreinotthere', true)).toBeNull();

		// Without a blob reader there's nothing to serve, and nothing to copy a photo with.
		expect(await blind().photo(epk, cid(wide), false)).toBeNull();
		expect(await blind().photo(epk, cid(secret), true)).toBeNull();
		await expect(blind().copyPhoto(f.t.net.client(ROOT), wide, { did: DJ })).rejects.toThrow(/no way to download/);
	});

	it("reads no private part when its space holds none, or one that doesn't validate", async () => {
		const epk = await f.publish(DJ);
		const root = f.t.net.client(ROOT);
		const at = { space: epk.space, collection: COLLECTION.epkContent, rkey: EPK_CONTENT_KEY };

		await root.putSpaceRecord({ ...at, record: { bio: 'No updatedAt' } });
		expect(await f.t.epks.content(epk)).toBeNull();

		await root.deleteSpaceRecord(at);
		expect(await f.t.epks.content(epk)).toBeNull();
	});

	it("skips a record in the root's repo that doesn't validate, and orders one never updated by when it was made", async () => {
		const clubs = await f.publish(DJ);
		const root = f.t.net.client(ROOT);
		const { updatedAt: _updatedAt, ...record } = f.t.net.records.get(`${ROOT}|${COLLECTION.epk}|${clubs.rkey}`)!.value;

		await root.putRecord({
			collection: COLLECTION.epk,
			rkey: TID.nextStr(),
			record: { ...record, title: 'Oldest', createdAt: '2020-01-01T00:00:00.000Z' }
		});
		await root.putRecord({ collection: COLLECTION.epk, rkey: TID.nextStr(), record: { ...record, title: '' } });
		await f.publish(CODJ, { title: 'Festivals' });
		f.t.epks.forget();

		expect((await f.t.epks.forProject(f.project)).map((epk) => epk.record.title)).toEqual([
			'Festivals',
			'Clubs',
			'Oldest'
		]);
	});

	it('deletes a press kit whose space is gone already, and revokes someone who had no grant', async () => {
		const epk = await f.publish(DJ);
		const kit = f.kit(epk);
		await f.t.epks.invite(await f.t.viewer(DJ), kit, FRIEND);

		await f.t.epks.revoke(await f.t.viewer(DJ), kit, STRANGER);
		expect((await f.t.epks.grants(epk)).map((grant) => grant.subject.did)).toEqual([FRIEND]);

		await f.t.net.client(ROOT).deleteSpace(epk.space);
		await f.t.epks.remove(await f.t.viewer(CODJ), kit);
		expect(f.t.net.records.has(`${ROOT}|${COLLECTION.epk}|${epk.rkey}`)).toBe(false);
		expect(await f.t.epks.forProject(f.project)).toEqual([]);
	});

	it("doesn't index a request in a press kit space under another account than the press kit's", async () => {
		const epk = await f.publish(DJ);
		const elsewhere = epkSpace(STRANGER, epk.rkey);

		await f.t.epks.indexRequest({
			space: elsewhere,
			uri: `${elsewhere}/${FRIEND}/${COLLECTION.epkRequest}/3req`,
			repoDid: FRIEND,
			value: { epk: epk.uri, createdAt: new Date().toISOString() }
		});
		expect(await f.t.epks.requests(epk)).toEqual([]);
	});
});
