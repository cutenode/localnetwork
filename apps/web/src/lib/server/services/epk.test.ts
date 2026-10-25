import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { z } from 'zod';
import { ACCESS_PRESETS, type EpkAccess } from '#lib/epk.ts';
import { COLLECTION, EPK_CONTENT_KEY } from '#lib/nsid.ts';
import type { ProjectView } from '#lib/types.ts';
import { recordCid } from '../atproto/repo-client';
import { ForbiddenError, ReauthError, UserError } from '../core';
import { HappyViewClient, HappyViewError } from '../happyview/client';
import { IndexedRepo } from '../happyview/indexed';
import { silentLogger } from '../logger';
import { createTestServices, ROOT } from '../testing/core';
import { EpkService, type EpkInput } from './epk';

const ADMIN = 'did:plc:admin';

const DJ = 'did:plc:dj';

const CODJ = 'did:plc:codj';

const PROMOTER = 'did:plc:promoter';

const FRIEND = 'did:plc:friend';

const STRANGER = 'did:plc:stranger';

const NO_SPACES = 'did:plc:nospaces';

const preset = (id: string): EpkAccess => ACCESS_PRESETS.find((entry) => entry.id === id)!.access;

const input = (overrides: Partial<EpkInput> = {}): EpkInput => ({
	title: 'Clubs',
	headline: 'Rolling techno for late rooms',
	bio: 'Short bio',
	access: preset('review'),
	mixes: [{ title: 'Public mix', url: 'https://soundcloud.com/dj/public' }],
	photos: [],
	content: {
		bio: 'The long bio',
		mixes: [{ title: 'Promo', url: 'https://soundcloud.com/dj/promo' }],
		contact: { email: 'book@dj.test' },
		techRider: '2x CDJ-3000'
	},
	...overrides
});

describe('EpkService', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;
	let project: ProjectView;

	beforeEach(async () => {
		t = await createTestServices({ admins: [ADMIN], capable: [ADMIN, DJ, CODJ, PROMOTER, FRIEND, STRANGER] });
		const admin = await t.viewer(ADMIN);

		const created = await t.projectAdmin.create({
			actor: admin,
			profile: { name: 'Neon Tide', kind: 'solo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } }
		});

		project = await t.projectAdmin.setManagers({ actor: admin, project: created, managers: [DJ, CODJ] });
	});
	afterEach(async () => {
		await t.audit.idle();
		await t.db.destroy();
	});

	const publish = async (overrides: Partial<EpkInput> = {}) =>
		t.epks.save(await t.viewer(DJ), project, input(overrides));

	/** A promoter with a booking thread for `uri`. */
	const book = async (did: string, uri: string) => {
		await t.bookings.start({ actor: await t.viewer(did), projectUri: uri, show: { title: 'Night' }, message: 'Hi' });
	};

	it("publishes several press kits, each public in the owner's repo with the rest in its own space", async () => {
		const clubs = await publish();
		const festivals = await publish({ title: 'Festivals' });

		expect((await t.epks.forProject(project)).map((epk) => epk.record.title).sort()).toEqual(['Clubs', 'Festivals']);
		expect(clubs.space).not.toBe(festivals.space);

		const published = t.net.records.get(`${DJ}|${COLLECTION.epk}|${clubs.rkey}`)!.value;
		expect(published.mixes).toEqual([{ title: 'Public mix', url: 'https://soundcloud.com/dj/public' }]);
		expect(JSON.stringify(published)).not.toContain('book@dj.test');

		const content = t.net.spaceRecords.get(`${clubs.space}|${DJ}|${COLLECTION.epkContent}|${EPK_CONTENT_KEY}`)!.value;
		expect(content).toMatchObject({ bio: 'The long bio', contact: { email: 'book@dj.test' } });
		expect(t.net.spaces.get(clubs.space)?.members.get(ROOT)).toEqual({ read: true, write: true });
	});

	it('asks an owner whose session lapsed to sign in again', async () => {
		// Spaces-capable when they signed in, with no OAuth session left to write with.
		const lapsed = { ...(await t.viewer(DJ)), did: 'did:plc:lapsed' };
		await t.projectAdmin.setManagers({ actor: await t.viewer(ADMIN), project, managers: [DJ, CODJ, lapsed.did] });
		const managed = (await t.projects.byUri(project.uri))!;

		await expect(t.epks.save(lapsed, managed, input())).rejects.toBeInstanceOf(ReauthError);
	});

	it('only counts press kits the project still lists the owner as managing', async () => {
		const epk = await publish();

		expect(await t.epks.get(DJ, epk.rkey)).not.toBeNull();
		await t.projectAdmin.setManagers({ actor: await t.viewer(ADMIN), project, managers: [CODJ] });
		expect(await t.epks.get(DJ, epk.rkey)).toBeNull();
		await expect(t.epks.save(await t.viewer(STRANGER), project, input())).rejects.toBeInstanceOf(ForbiddenError);
	});

	it("keeps the private part from people until the owner approves them, then writes the grant to the owner's PDS", async () => {
		const epk = await publish();
		const stranger = await t.viewer(STRANGER);

		expect(await t.epks.accessState(epk, null)).toEqual({ kind: 'signedOut', rule: 'review' });
		expect(await t.epks.accessState(epk, stranger)).toEqual({ kind: 'canAsk', rule: 'review' });

		const state = await t.epks.request(stranger, epk, { role: 'press', message: 'For a feature' });
		expect(state.kind).toBe('pending');
		const [request] = await t.epks.requests(epk);
		expect(request).toMatchObject({ role: 'press', message: 'For a feature', status: 'pending', rule: 'review' });
		expect(t.net.spaceRecordsIn(epk.space, STRANGER)).toHaveLength(1);

		await expect(t.epks.decide(stranger, epk, request.uri, 'approve')).rejects.toBeInstanceOf(ForbiddenError);
		await t.epks.decide(await t.viewer(DJ), epk, request.uri, 'approve');

		expect((await t.epks.accessState(epk, stranger)).kind).toBe('granted');
		const grant = t.net.spaceRecords.get(`${epk.space}|${DJ}|${COLLECTION.epkGrant}|${STRANGER}`)!.value;
		expect(grant).toMatchObject({ subject: STRANGER, via: 'approved', request: request.uri });
		expect(t.net.spaces.get(epk.space)?.members.get(STRANGER)).toEqual({ read: true, write: false });
		expect((await t.epks.content(epk))?.techRider).toBe('2x CDJ-3000');
	});

	it('approves at once when the rules say so', async () => {
		const epk = await publish({ access: preset('open') });

		const state = await t.epks.request(await t.viewer(STRANGER), epk, {});

		expect(state.kind).toBe('granted');
		expect(t.net.spaceRecords.get(`${epk.space}|${DJ}|${COLLECTION.epkGrant}|${STRANGER}`)?.value.via).toBe('auto');
		expect(await t.epks.requests(epk)).toEqual([]);
	});

	it('writes an automatic approval once the owner is back when their session had lapsed', async () => {
		const epk = await publish({ access: preset('open') });
		t.capable.delete(DJ);

		expect((await t.epks.request(await t.viewer(STRANGER), epk, {})).kind).toBe('pending');
		expect(await t.epks.waitingFor(DJ)).toBe(0);

		t.capable.add(DJ);
		expect(await t.epks.settle(await t.viewer(DJ))).toBe(1);
		expect((await t.epks.accessState(epk, await t.viewer(STRANGER))).kind).toBe('granted');
	});

	it('reads each audience from bookings and follows', async () => {
		const access: EpkAccess = { bookedWith: 'auto', following: 'auto', promoters: 'review', anyone: 'none' };
		const epk = await publish({ access });

		const other = await t.projectAdmin.create({
			actor: await t.viewer(ADMIN),
			profile: { name: 'Elsewhere', kind: 'solo', genres: ['house'], location: { city: 'Leeds', country: 'GB' } }
		});

		const claimed = await t.projectAdmin.setManagers({
			actor: await t.viewer(ADMIN),
			project: other,
			managers: [CODJ]
		});

		await book(PROMOTER, claimed.uri);
		await book(FRIEND, project.uri);
		t.net.records.set(`${DJ}|app.bsky.graph.follow|3follow`, {
			uri: `at://${DJ}/app.bsky.graph.follow/3follow`,
			cid: 'c',
			value: { $type: 'app.bsky.graph.follow', subject: NO_SPACES, createdAt: '2026-01-01T00:00:00Z' }
		});

		expect(await t.epks.ruleForViewer(epk, FRIEND)).toBe('auto');
		expect(await t.epks.ruleForViewer(epk, NO_SPACES)).toBe('auto');
		expect(await t.epks.ruleForViewer(epk, PROMOTER)).toBe('review');
		expect(await t.epks.accessState(epk, await t.viewer(STRANGER))).toEqual({ kind: 'closed' });
		await expect(t.epks.request(await t.viewer(STRANGER), epk, {})).rejects.toBeInstanceOf(UserError);
	});

	it('takes no requests when nobody can ask, but the owner can still add people', async () => {
		const epk = await publish({ access: preset('closed') });
		const owner = await t.viewer(DJ);

		expect(await t.epks.accessState(epk, null)).toEqual({ kind: 'closed' });
		await t.epks.invite(owner, epk, FRIEND);
		expect(t.net.spaceRecords.get(`${epk.space}|${DJ}|${COLLECTION.epkGrant}|${FRIEND}`)?.value.via).toBe('invited');
		expect((await t.epks.grants(epk)).map((grant) => grant.via)).toEqual(['invited']);

		await t.epks.revoke(owner, epk, FRIEND);
		expect(t.net.spaceRecords.has(`${epk.space}|${DJ}|${COLLECTION.epkGrant}|${FRIEND}`)).toBe(false);
		expect(t.net.spaces.get(epk.space)?.members.has(FRIEND)).toBe(false);
		expect(await t.epks.accessState(epk, await t.viewer(FRIEND))).toEqual({ kind: 'closed' });
	});

	it('tells someone who was declined, and lets nobody ask twice', async () => {
		const epk = await publish();
		const stranger = await t.viewer(STRANGER);

		await t.epks.request(stranger, epk, {});
		await expect(t.epks.request(stranger, epk, {})).rejects.toThrow(/already asked/);
		const [request] = await t.epks.requests(epk);
		await t.epks.decide(await t.viewer(DJ), epk, request.uri, 'decline');

		expect(await t.epks.accessState(epk, stranger)).toEqual({ kind: 'declined' });
		await expect(t.epks.decide(await t.viewer(DJ), epk, request.uri, 'approve')).rejects.toThrow(
			/already been answered/
		);
	});

	it("writes the request for someone whose PDS can't, on their behalf", async () => {
		const epk = await publish();

		await t.epks.request(await t.viewer(NO_SPACES), epk, {});

		const [written] = t.net.spaceRecordsIn(epk.space, ROOT);
		expect(written.value).toMatchObject({ epk: epk.uri, onBehalfOf: NO_SPACES });
		expect((await t.epks.requests(epk))[0].requester.did).toBe(NO_SPACES);
	});

	it("deletes a press kit with its space, and everyone's access with it", async () => {
		const epk = await publish({ access: preset('open') });
		await t.epks.request(await t.viewer(STRANGER), epk, {});

		await expect(t.epks.remove(await t.viewer(CODJ), epk)).rejects.toBeInstanceOf(ForbiddenError);
		await t.epks.remove(await t.viewer(DJ), epk);

		expect(t.net.spaces.has(epk.space)).toBe(false);
		expect(await t.epks.forProject(project)).toEqual([]);
		expect(await t.epks.grants(epk)).toEqual([]);
	});

	it('counts the press kits a project has from HappyView, up to 20', async () => {
		for (let i = 0; i < 20; i++) await publish({ title: `Kit ${i}` });

		await expect(publish()).rejects.toEqual(new UserError('A project can have at most 20 press kits'));
		expect(await t.epks.forProject(project)).toHaveLength(20);
	});

	it('saves only on the version it was opened on, and asks to reload otherwise', async () => {
		const backfilled: string[] = [];

		// The app's own HappyView, with an API key, so a stale press kit is re-read.
		const admin = new HappyViewClient({
			url: 'http://happyview.test',
			apiKey: 'hv_test',
			fetch: async (url, init) => {
				const request = new Request(url, init);

				if (new URL(request.url).pathname !== '/admin/backfill') return t.happyView.fetch(request);
				const { collection, did } = z.object({ collection: z.string(), did: z.string() }).parse(await request.json());
				backfilled.push(`${collection} ${did}`);

				return Response.json({ id: 'job', status: 'running' });
			}
		});

		const epks = new EpkService(
			t.core,
			t.projects,
			new IndexedRepo(admin, silentLogger, 0),
			t.publicRepo,
			async (id) => id
		);

		const owner = await t.viewer(DJ);
		const changed = new UserError('This press kit changed since you opened it. Reload and try again.', 409);
		const epk = await epks.save(owner, project, input());
		const key = `${DJ}|${COLLECTION.epk}|${epk.rkey}`;
		const content = `${epk.space}|${DJ}|${COLLECTION.epkContent}|${EPK_CONTENT_KEY}`;

		// Saved on the version it was opened on, it lands, as a new version.
		const edited = await epks.save(owner, project, input({ title: 'Clubs, edited' }), epk);
		expect((await epks.get(DJ, epk.rkey))?.epk).toMatchObject({ cid: edited.cid, record: { title: 'Clubs, edited' } });
		expect(edited.cid).not.toBe(epk.cid);

		// A form still open on the first version is refused before it writes anything, the private part included.
		const privatePart = t.net.spaceRecords.get(content);
		await expect(epks.save(owner, project, input({ title: 'Stale', bio: 'Stale bio' }), epk)).rejects.toEqual(changed);
		await expect(epks.save(owner, project, input(), { rkey: epk.rkey, cid: '' })).rejects.toEqual(changed);
		expect(t.net.records.get(key)?.value.title).toBe('Clubs, edited');
		expect(t.net.spaceRecords.get(content)).toEqual(privatePart);
		expect(backfilled).toEqual([]);

		// Another app changes it in the owner's repo, and HappyView hasn't heard yet: the PDS refuses the swap.
		const elsewhere = { ...t.net.records.get(key)!.value, title: 'Changed elsewhere' };
		t.net.records.set(key, { uri: epk.uri, cid: await recordCid(elsewhere), value: elsewhere });

		await expect(epks.save(owner, project, input({ title: 'Mine' }), edited)).rejects.toEqual(changed);
		expect(t.net.records.get(key)?.value.title).toBe('Changed elsewhere');
		expect(backfilled).toEqual([`${COLLECTION.epk} ${DJ}`]);
		await expect(epks.save(owner, project, input(), { rkey: 'missing', cid: edited.cid })).rejects.toThrow(
			/does not exist/
		);
	});

	it("won't save while HappyView is down, rather than taking a press kit for missing or counting none", async () => {
		const epk = await publish();

		const down = new EpkService(
			t.core,
			t.projects,
			new IndexedRepo(
				new HappyViewClient({ url: 'http://happyview.test', fetch: async () => new Response('down', { status: 503 }) }),
				silentLogger,
				0
			),
			t.publicRepo,
			async (id) => id
		);

		const owner = await t.viewer(DJ);
		const before = t.net.records.get(`${DJ}|${COLLECTION.epk}|${epk.rkey}`);
		const written = t.net.records.size;

		await expect(down.save(owner, project, input({ title: 'Edited' }), epk)).rejects.toBeInstanceOf(HappyViewError);
		await expect(down.save(owner, project, input())).rejects.toBeInstanceOf(HappyViewError);
		await expect(down.get(DJ, epk.rkey)).rejects.toBeInstanceOf(HappyViewError);
		expect(t.net.records.get(`${DJ}|${COLLECTION.epk}|${epk.rkey}`)).toEqual(before);
		expect(t.net.records.size).toBe(written);
		// Lists still render without it.
		expect(await down.forProject(project)).toEqual([]);
	});

	it("won't publish from a PDS without spaces", async () => {
		await expect(t.epks.save(await t.viewer(NO_SPACES), project, input())).rejects.toBeInstanceOf(ForbiddenError);
		t.capable.delete(CODJ);
		await expect(t.epks.save(await t.viewer(CODJ), project, input())).rejects.toThrow(/spaces/);
	});
});
