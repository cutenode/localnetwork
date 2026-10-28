import type { JsonValue } from '@atproto/lex-json';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { sampleReport } from '#lib/mix/sample.ts';
import { COLLECTION } from '#lib/nsid.ts';
import type { ProjectProfile } from '#lib/types.ts';
import { plannerSpace } from '../atproto/spaces';
import { ForbiddenError, NotFoundError, UserError } from '../core';
import { HappyViewClient } from '../happyview/client';
import { createTestServices, ROOT } from '../testing/core';
import { ProjectService, slugify } from './projects';

const ADMIN = 'did:plc:admin';

const MOD = 'did:plc:mod';

const DJ = 'did:plc:dj';

const PROMOTER = 'did:plc:promoter';

const LEGACY = 'did:plc:legacy';

const profile = (name: string, overrides: Partial<ProjectProfile> = {}): ProjectProfile => ({
	name,
	kind: 'solo',
	genres: ['house'],
	location: { city: 'Berlin', country: 'DE', latE6: 52_520_000, lngE6: 13_405_000 },
	...overrides
});

describe('services', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;

	beforeEach(async () => {
		t = await createTestServices({ admins: [ADMIN], capable: [ADMIN, MOD, DJ, PROMOTER] });
		await t.staff.grant(await t.viewer(ADMIN), MOD, 'moderator');
	});
	afterEach(async () => {
		await t.audit.idle();
		await t.db.destroy();
	});

	describe('ProjectService', () => {
		it('slugifies names', () => {
			expect(slugify('Saffron & Sons!')).toBe('saffron-and-sons');
			expect(slugify('Ñoño')).toBe('nono');
			expect(slugify('!')).toBe('project-x');
		});

		it('publishes with compare-and-swap, unique slugs, and private details', async () => {
			const admin = await t.viewer(ADMIN);

			const first = await t.projectAdmin.create({
				actor: admin,
				profile: profile('Echo'),
				details: { bookingEmail: 'a@b.test' }
			});

			const second = await t.projectAdmin.create({ actor: admin, profile: profile('Echo') });
			expect([first.slug, second.slug]).toEqual(['echo', 'echo-2']);
			const stored = t.net.records.get(`${ROOT}|${COLLECTION.project}|${first.rkey}`);
			expect(stored?.value).toMatchObject({ slug: 'echo', revision: 1, status: 'active' });
			expect(await t.projects.details(first.uri)).toEqual({ bookingEmail: 'a@b.test' });

			const updated = await t.projectAdmin.update({
				actor: admin,
				project: first,
				profile: profile('Echo', { tagline: 'New' })
			});

			expect(updated.revision).toBe(2);
			await expect(t.projectAdmin.update({ actor: admin, project: first, profile: profile('Echo') })).rejects.toThrow(
				/changed while you were editing/
			);
		});

		it('restricts direct edits to admins and records denials', async () => {
			await expect(
				t.projectAdmin.create({ actor: await t.viewer(MOD), profile: profile('Nope') })
			).rejects.toBeInstanceOf(ForbiddenError);
			expect((await t.audit.list({ outcome: 'denied' })).length).toBe(1);
		});

		it('searches by text, distance, genre family, filters, and sorts', async () => {
			const admin = await t.viewer(ADMIN);
			await t.projectAdmin.create({
				actor: admin,
				profile: profile('Deep Current', {
					genres: ['deep house'],
					fees: [{ tier: 'emerging' }],
					formats: ['vinyl'],
					setTypes: ['warmup']
				})
			});
			await t.projectAdmin.create({
				actor: admin,
				profile: profile('Iron Pulse', {
					genres: ['hard techno'],
					location: { city: 'Hamburg', country: 'DE', latE6: 53_551_000, lngE6: 9_993_000 }
				})
			});
			await t.projectAdmin.create({
				actor: admin,
				profile: profile('No Coords', { genres: ['house'], location: { city: 'Somewhere', country: 'US' } })
			});

			expect((await t.projects.search({ q: 'current' })).items.map((p) => p.profile.name)).toEqual(['Deep Current']);
			expect((await t.projects.search({ q: 'zzz' })).total).toBe(0);
			// Lexicons count UTF-8 bytes: 80 two-byte characters must be cut to fit, not rejected.
			expect((await t.projects.search({ q: 'é'.repeat(80) })).total).toBe(0);
			expect((await t.projects.search({ genres: ['house'] })).items.map((p) => p.profile.name).sort()).toEqual([
				'Deep Current',
				'No Coords'
			]);
			const near = await t.projects.search({ near: { lat: 52.52, lng: 13.4 }, radiusKm: 50 });
			expect(near.items.map((p) => p.profile.name)).toEqual(['Deep Current']);
			expect(near.items[0].distanceKm).toBeLessThan(1);
			const wide = await t.projects.search({ near: { lat: 52.52, lng: 13.4 }, radiusKm: 400, sort: 'distance' });
			expect(wide.items.map((p) => p.profile.name)).toEqual(['Deep Current', 'Iron Pulse']);
			expect((await t.projects.search({ feeTiers: ['emerging'] })).total).toBe(1);
			expect((await t.projects.search({ formats: ['vinyl'], setTypes: ['warmup'] })).total).toBe(1);
			expect((await t.projects.search({ country: 'us' })).total).toBe(1);
			expect((await t.projects.search({ unclaimed: true })).total).toBe(3);
			expect(
				(await t.projects.search({ sort: 'name', pageSize: 2, page: 2 })).items.map((p) => p.profile.name)
			).toEqual(['No Coords']);
			expect((await t.projects.stats()).genres[0]).toMatchObject({ count: 1 });
			expect(await t.projects.stats()).toMatchObject({ total: 3, active: 3, unclaimed: 3 });
		});

		it('ranks matches and words their reasons', async () => {
			const admin = await t.viewer(ADMIN);

			const project = await t.projectAdmin.create({
				actor: admin,
				profile: profile('Matcher', { genres: ['techno'] })
			});

			const results = await t.projects.match({
				location: { lat: 52.52, lng: 13.405 },
				radiusKm: 25,
				genres: ['techno']
			});

			expect(results[0].candidate.uri).toBe(project.uri);
			expect(results[0].score).toBeGreaterThan(90);
			expect(results[0].breakdown).toMatchObject({ genre: 100, distance: 100 });
			expect(results[0].reasons).toEqual(['Plays techno', 'Local']);
		});

		it('lists managed and recent projects, stats, and lookups by URI', async () => {
			const admin = await t.viewer(ADMIN);
			const mine = await t.projectAdmin.create({ actor: admin, profile: profile('Zephyr'), managers: [DJ] });
			const hidden = await t.projectAdmin.create({ actor: admin, profile: profile('Aurora'), managers: [DJ] });
			const other = await t.projectAdmin.create({ actor: admin, profile: profile('Other', { genres: ['disco'] }) });
			await t.projectAdmin.setStatus({ actor: admin, project: hidden, status: 'hidden' });

			expect((await t.projects.managedBy(DJ)).map((p) => p.profile.name)).toEqual(['Aurora', 'Zephyr']);
			expect((await t.projects.recent(2)).map((p) => p.profile.name)).toEqual(['Other', 'Zephyr']);
			expect(await t.projects.stats()).toMatchObject({
				total: 3,
				active: 2,
				unclaimed: 1,
				cityCount: 1,
				genres: [
					{ value: 'disco', count: 1 },
					{ value: 'house', count: 1 }
				],
				cities: [{ city: 'Berlin', country: 'DE', count: 2 }]
			});

			// 32 root project URIs take two getProjects calls. The rest can't be the root's projects and aren't sent.
			const unknown = Array.from({ length: 30 }, (_, i) => `at://${ROOT}/${COLLECTION.project}/unknown${i}`);
			const found = await t.projects.byUris([...unknown, mine.uri, other.uri, 'not a uri', `at://${DJ}/x/y`]);
			expect([...found.keys()].sort()).toEqual([mine.uri, other.uri].sort());
			expect(await t.projects.byUri(`at://${DJ}/${COLLECTION.project}/${mine.rkey}`)).toBeNull();
			expect(await t.projects.bySlug('Not A Slug')).toBeNull();
		});

		it('only lists projects the root account published', async () => {
			await t.projectAdmin.create({ actor: await t.viewer(ADMIN), profile: profile('Official') });
			const stored = [...t.net.records.values()][0];

			await t.net.client(DJ).putRecord({ collection: COLLECTION.project, rkey: 'mine', record: stored.value });

			expect((await t.projects.search({})).items.map((p) => p.uri)).toEqual([stored.uri]);
		});

		it('waits for the directory to serve a write, and answers without it when it lags', async () => {
			const ingest = t.net.onRecord;

			// Jetstream delivers a moment after the write.
			t.net.onRecord = async (event) => {
				setTimeout(() => void ingest(event), 30);
			};

			const prompt = await t.projectAdmin.create({ actor: await t.viewer(ADMIN), profile: profile('Prompt') });
			expect((await t.projects.byUri(prompt.uri))?.cid).toBe(prompt.cid);

			const lagging = new ProjectService(t.core, t.directory, 50);
			t.net.onRecord = async () => {};

			const { project } = await lagging.publish({
				profile: profile('Late Night'),
				managers: [DJ],
				status: 'active',
				createdBy: ADMIN
			});

			expect(project).toMatchObject({
				slug: 'late-night',
				revision: 1,
				managers: [DJ],
				profile: { name: 'Late Night' }
			});
			expect(project.cid).toBe(t.net.records.get(`${ROOT}|${COLLECTION.project}|${project.rkey}`)?.cid);
			expect(await lagging.byUri(project.uri)).toBeNull();
		});

		it('skips directory projects it cannot vouch for', async () => {
			const project = await t.projectAdmin.create({ actor: await t.viewer(ADMIN), profile: profile('Vouched') });
			const { value } = t.net.records.get(`${ROOT}|${COLLECTION.project}|${project.rkey}`)!;

			const serving = (uri: string, record: JsonValue) =>
				new ProjectService(
					t.core,
					new HappyViewClient({
						url: 'http://happyview.test',
						fetch: async () => Response.json({ project: { uri, cid: project.cid, record } })
					})
				);

			expect(await serving(project.uri, value).byUri(project.uri)).toMatchObject({ slug: 'vouched' });
			expect(await serving(project.uri, { ...value, status: 'paused' }).byUri(project.uri)).toBeNull();
			expect(await serving(project.uri, { ...value, profile: { name: 'No kind' } }).byUri(project.uri)).toBeNull();
			expect(await serving(`at://${DJ}/${COLLECTION.project}/x`, value).bySlug('vouched')).toBeNull();
		});
	});

	describe('ProjectAdmin', () => {
		it('lets staff hide projects, but only admins archive them or change managers', async () => {
			const admin = await t.viewer(ADMIN);
			const mod = await t.viewer(MOD);
			const project = await t.projectAdmin.create({ actor: admin, profile: profile('Quiet') });

			await expect(
				t.projectAdmin.setStatus({ actor: await t.viewer(DJ), project, status: 'hidden' })
			).rejects.toBeInstanceOf(ForbiddenError);
			await expect(t.projectAdmin.setStatus({ actor: mod, project, status: 'archived' })).rejects.toThrow(
				/Only admins/
			);
			const hidden = await t.projectAdmin.setStatus({ actor: mod, project, status: 'hidden', reason: 'Spam' });
			expect(hidden.status).toBe('hidden');
			expect(await t.projectAdmin.setStatus({ actor: mod, project: hidden, status: 'hidden' })).toBe(hidden);
			expect((await t.projects.search({})).total).toBe(0);

			await expect(t.projectAdmin.setManagers({ actor: mod, project: hidden, managers: [DJ] })).rejects.toBeInstanceOf(
				ForbiddenError
			);
			await expect(t.projectAdmin.setManagers({ actor: admin, project: hidden, managers: ['dj'] })).rejects.toThrow(
				/DIDs/
			);
			const crowd = Array.from({ length: 11 }, (_, i) => `did:plc:m${i}`);
			await expect(t.projectAdmin.setManagers({ actor: admin, project: hidden, managers: crowd })).rejects.toThrow(
				/at most 10/
			);
			const managed = await t.projectAdmin.setManagers({ actor: admin, project: hidden, managers: [DJ, DJ, PROMOTER] });
			expect(managed.managers).toEqual([DJ, PROMOTER]);
			expect((await t.projects.managedBy(PROMOTER)).map((p) => p.uri)).toEqual([project.uri]);
			expect((await t.audit.list({ subject: project.uri, outcome: 'denied' })).map((e) => e.action)).toEqual([
				'project.managers'
			]);
		});

		it('keeps, replaces, or removes the avatar on direct edits, and uploads tracklists', async () => {
			const admin = await t.viewer(ADMIN);
			const png = (n: number) => ({ bytes: new Uint8Array([0x89, 0x50, 0x4e, 0x47, n]), mimeType: 'image/png' });
			const project = await t.projectAdmin.create({ actor: admin, profile: profile('Pictured'), avatar: png(1) });
			const first = project.profile.avatar?.ref.$link;
			expect(project.avatarUrl).toBe(`/media/${ROOT}/${first}`);

			const kept = await t.projectAdmin.update({
				actor: admin,
				project,
				profile: profile('Pictured', { tagline: 'New' })
			});

			expect(kept.profile.avatar?.ref.$link).toBe(first);

			const replaced = await t.projectAdmin.update({
				actor: admin,
				project: kept,
				profile: kept.profile,
				avatar: png(2)
			});

			expect(replaced.profile.avatar?.ref.$link).not.toBe(first);

			const removed = await t.projectAdmin.update({
				actor: admin,
				project: replaced,
				profile: profile('Pictured'),
				removeAvatar: true
			});

			expect(removed.avatarUrl).toBeNull();

			const mixes = [
				{ title: 'Plain', url: 'https://mixes.test/plain' },
				{ title: 'Listed', url: 'https://mixes.test/listed' }
			];

			const taped = await t.projectAdmin.create({
				actor: admin,
				profile: profile('Taped', { mixes }),
				tracklists: [undefined, { bytes: new TextEncoder().encode('{}'), mimeType: 'application/json' }]
			});

			expect(taped.profile.mixes?.map((mix) => mix.tracklist?.mimeType)).toEqual([undefined, 'application/json']);
		});
	});

	describe('RequestService', () => {
		it('runs a create → approve flow with avatar copy and decision record', async () => {
			const dj = await t.viewer(DJ);
			const png = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 1, 2, 3]);

			const request = await t.requests.submit({
				actor: dj,
				kind: 'create',
				profile: profile('Glow'),
				avatar: { bytes: png, mimeType: 'image/png' }
			});

			expect(request.repoDid).toBe(DJ);
			expect(request.record.profile?.avatar?.mimeType).toBe('image/png');
			const decided = await t.requests.decide({ actor: await t.viewer(MOD), uri: request.uri, outcome: 'approved' });
			expect(decided.status).toBe('approved');
			expect(decided.resultProjectSlug).toBe('glow');
			const project = await t.projects.bySlug('glow');
			expect(project?.managers).toEqual([DJ]);
			expect(project?.avatarUrl).toMatch(/^\/media\/did:plc:root\//);

			const decision = t.net
				.spaceRecordsIn(t.spaces.moderation, ROOT)
				.find((record) => record.uri.includes(COLLECTION.decision));

			expect(decision?.value).toMatchObject({ outcome: 'approved', decidedBy: MOD, requester: DJ });
		});

		it('copies proposed tracklists on approval and keeps published files on updates', async () => {
			const dj = await t.viewer(DJ);
			const mod = await t.viewer(MOD);

			const json = (value: JsonValue) => ({
				bytes: new TextEncoder().encode(JSON.stringify(value)),
				mimeType: 'application/json'
			});

			const mixes = [
				{ title: 'Warehouse', url: 'https://mixes.test/warehouse' },
				{ title: 'Broken', url: 'https://mixes.test/broken' },
				{ title: 'Plain', url: 'https://mixes.test/plain' }
			];

			const request = await t.requests.submit({
				actor: dj,
				kind: 'create',
				profile: profile('Tracks', { mixes }),
				avatar: { bytes: new Uint8Array([0x89, 0x50, 0x4e, 0x47, 1]), mimeType: 'image/png' },
				tracklists: [json({ ...sampleReport, extra: true }), json({ format: 'nope' })]
			});

			const proposed = request.record.profile?.mixes?.[0]?.tracklist;
			expect(proposed?.mimeType).toBe('application/json');
			expect((await t.tracklists.proposed(DJ, proposed!)).mix.title).toBe(sampleReport.mix.title);

			await t.requests.decide({ actor: mod, uri: request.uri, outcome: 'approved' });
			const project = (await t.projects.bySlug('tracks'))!;
			const [warehouse, broken, plain] = project.profile.mixes ?? [];
			// Re-serialized on the way to root, so unknown fields are gone and the CID changes.
			expect(warehouse.tracklist?.ref.$link).not.toBe(proposed?.ref.$link);
			const published = JSON.parse(new TextDecoder().decode(t.net.blobs.get(warehouse.tracklist!.ref.$link)));
			expect(published).not.toHaveProperty('extra');
			expect((await t.tracklists.published(warehouse.tracklist!)).plays).toHaveLength(sampleReport.plays.length);
			// An invalid upload is dropped rather than published.
			expect(broken.tracklist).toBeUndefined();
			expect(plain.tracklist).toBeUndefined();

			const fetchSpaceBlob = vi.spyOn(t.core, 'fetchSpaceBlob');

			const update = await t.requests.submit({
				actor: dj,
				kind: 'update',
				projectUri: project.uri,
				profile: { ...project.profile, tagline: 'Now with tracklists' }
			});

			await t.requests.decide({ actor: mod, uri: update.uri, outcome: 'approved' });
			expect(fetchSpaceBlob).not.toHaveBeenCalled();
			const updated = (await t.projects.bySlug('tracks'))!;
			expect(updated.profile.tagline).toBe('Now with tracklists');
			expect(updated.profile.avatar?.ref.$link).toBe(project.profile.avatar?.ref.$link);
			expect(updated.profile.mixes?.[0]?.tracklist?.ref.$link).toBe(warehouse.tracklist?.ref.$link);
		});

		it('validates submissions', async () => {
			const dj = await t.viewer(DJ);
			await expect(t.requests.submit({ actor: dj, kind: 'create' })).rejects.toThrow(/profile/);
			await expect(
				t.requests.submit({
					actor: dj,
					kind: 'claim',
					projectUri: 'at://x/y/z',
					claim: { relationship: 'self', evidence: 'x' }
				})
			).rejects.toBeInstanceOf(NotFoundError);
			const project = await t.projectAdmin.create({ actor: await t.viewer(ADMIN), profile: profile('Claimable') });
			await expect(
				t.requests.submit({
					actor: dj,
					kind: 'claim',
					projectUri: project.uri,
					claim: { relationship: 'self', evidence: ' ' }
				})
			).rejects.toThrow(/verify/);
			await expect(
				t.requests.submit({ actor: dj, kind: 'update', projectUri: project.uri, profile: profile('X') })
			).rejects.toBeInstanceOf(ForbiddenError);

			for (let i = 0; i < 5; i++) await t.requests.submit({ actor: dj, kind: 'create', profile: profile(`P${i}`) });
			await expect(t.requests.submit({ actor: dj, kind: 'create', profile: profile('Too many') })).rejects.toThrow(
				/already have 5/
			);
		});

		it('allows one open change request per manager and project', async () => {
			const dj = await t.viewer(DJ);

			const project = await t.projectAdmin.create({
				actor: await t.viewer(ADMIN),
				profile: profile('Once'),
				managers: [DJ]
			});

			const first = await t.requests.submit({
				actor: dj,
				kind: 'update',
				projectUri: project.uri,
				profile: profile('A')
			});

			expect((await t.requests.openUpdate(project.uri, DJ))?.uri).toBe(first.uri);
			expect(await t.requests.openUpdate(project.uri, PROMOTER)).toBeNull();
			await expect(
				t.requests.submit({ actor: dj, kind: 'update', projectUri: project.uri, profile: profile('B') })
			).rejects.toThrow(/already have changes waiting/);
			await t.requests.withdraw(dj, first.uri);
			expect(await t.requests.openUpdate(project.uri, DJ)).toBeNull();
			await t.requests.submit({ actor: dj, kind: 'update', projectUri: project.uri, profile: profile('B') });
		});

		it('clears private details through a change request', async () => {
			const dj = await t.viewer(DJ);

			const project = await t.projectAdmin.create({
				actor: await t.viewer(ADMIN),
				profile: profile('Private'),
				details: { bookingEmail: 'a@b.test' },
				managers: [DJ]
			});

			const update = await t.requests.submit({
				actor: dj,
				kind: 'update',
				projectUri: project.uri,
				profile: project.profile,
				details: {}
			});

			await t.requests.decide({ actor: await t.viewer(MOD), uri: update.uri, outcome: 'approved' });
			expect(await t.projects.details(project.uri)).toEqual({});
		});

		it('supports rejection, withdrawal, and blocks double decisions', async () => {
			const dj = await t.viewer(DJ);
			const mod = await t.viewer(MOD);
			const request = await t.requests.submit({ actor: dj, kind: 'create', profile: profile('Rejected') });
			await expect(t.requests.decide({ actor: mod, uri: request.uri, outcome: 'rejected' })).rejects.toThrow(/reason/);
			await t.requests.decide({ actor: mod, uri: request.uri, outcome: 'rejected', reason: 'Duplicate' });
			await expect(t.requests.decide({ actor: mod, uri: request.uri, outcome: 'approved' })).rejects.toThrow(
				/not waiting/
			);
			await expect(t.requests.withdraw(dj, request.uri)).rejects.toThrow(/already been decided/);

			const other = await t.requests.submit({ actor: dj, kind: 'create', profile: profile('Withdrawn') });
			await expect(t.requests.withdraw(await t.viewer(PROMOTER), other.uri)).rejects.toBeInstanceOf(ForbiddenError);
			await t.requests.withdraw(dj, other.uri);
			expect((await t.requests.counts()).withdrawn).toBe(1);
			expect((await t.requests.list({ requester: DJ })).length).toBe(2);
		});

		it('keeps the message through a revision unless it is left blank', async () => {
			const dj = await t.viewer(DJ);

			const request = await t.requests.submit({
				actor: dj,
				kind: 'create',
				profile: profile('Hush'),
				message: 'First take'
			});

			const kept = await t.requests.revise(dj, request.uri, { profile: profile('Hush', { tagline: 'Quiet' }) });
			expect(kept.record).toMatchObject({ message: 'First take', profile: { tagline: 'Quiet' } });
			const cleared = await t.requests.revise(dj, request.uri, { message: '  ' });
			const written = t.net.spaceRecordsIn(t.spaces.moderation, DJ).find((record) => record.uri === request.uri);
			expect(Object.keys(cleared.record)).not.toContain('message');
			expect(Object.keys(written?.value ?? {})).not.toContain('message');
			expect(written?.value).toMatchObject({ profile: { tagline: 'Quiet' } });
		});

		it('files requests on behalf of accounts without spaces support', async () => {
			const legacy = await t.viewer(LEGACY);
			const request = await t.requests.submit({ actor: legacy, kind: 'create', profile: profile('Proxy') });
			expect(request).toMatchObject({ repoDid: ROOT, requester: { did: LEGACY }, record: { onBehalfOf: LEGACY } });
			await t.audit.idle();
			expect((await t.audit.list({ actor: LEGACY }))[0].actorStatus).toBe('unsupported');
		});

		it('ignores onBehalfOf claims from anyone but the root when indexing', async () => {
			await t.requests.index({
				uri: `${t.spaces.moderation}/${PROMOTER}/${COLLECTION.request}/3zzz`,
				cid: 'c',
				repoDid: PROMOTER,
				value: { kind: 'create', onBehalfOf: DJ, createdAt: new Date().toISOString() }
			});
			expect((await t.requests.byRkey('3zzz'))?.requester.did).toBe(PROMOTER);
		});
	});

	describe('StaffService', () => {
		it('grants, lists, and revokes roles with safety rails', async () => {
			const admin = await t.viewer(ADMIN);
			expect(await t.staff.roles(ADMIN)).toEqual(['admin']);
			expect(await t.staff.roles(MOD)).toEqual(['moderator']);
			const members = await t.staff.list();
			expect(members.map((m) => [m.did, m.role, m.bootstrap])).toEqual([
				[ADMIN, 'admin', true],
				[MOD, 'moderator', false]
			]);
			expect(t.net.spaces.get(t.spaces.audit)?.members.get(MOD)).toEqual({ read: true, write: true });
			await expect(t.staff.grant(await t.viewer(MOD), DJ, 'admin')).rejects.toBeInstanceOf(ForbiddenError);
			await expect(t.staff.grant(admin, ADMIN, 'moderator')).rejects.toBeInstanceOf(UserError);
			await expect(t.staff.revoke(admin, ADMIN)).rejects.toThrow(/ADMIN_DIDS/);
			await expect(t.staff.revoke(admin, DJ)).rejects.toThrow(/not staff/);
			await t.staff.revoke(admin, MOD);
			expect(await t.staff.roles(MOD)).toEqual([]);
			expect(t.net.spaces.get(t.spaces.moderation)?.members.has(MOD)).toBe(false);
			expect((await t.audit.list({ action: 'role' })).map((e) => e.action)).toEqual(['role.revoke', 'role.grant']);
		});
	});

	describe('BookingService and PlannerService', () => {
		it('runs a booking thread between a promoter and managers', async () => {
			const project = await t.projectAdmin.create({
				actor: await t.viewer(ADMIN),
				profile: profile('Bookable'),
				managers: [DJ]
			});

			const unclaimed = await t.projectAdmin.create({ actor: await t.viewer(ADMIN), profile: profile('Nobody') });
			const promoter = await t.viewer(PROMOTER);
			await expect(
				t.bookings.start({ actor: promoter, projectUri: unclaimed.uri, show: { title: 'x' }, message: 'hi' })
			).rejects.toThrow(/claimed/);
			await expect(
				t.bookings.start({ actor: await t.viewer(DJ), projectUri: project.uri, show: { title: 'x' }, message: 'hi' })
			).rejects.toThrow(/own project/);

			const space = await t.bookings.start({
				actor: promoter,
				projectUri: project.uri,
				show: { title: 'Night' },
				message: 'Free?'
			});

			await t.bookings.message({ actor: await t.viewer(DJ), space, text: 'Yes' });
			await t.bookings.setStatus({ actor: promoter, space, status: 'confirmed', note: 'Booked' });
			await expect(t.bookings.setStatus({ actor: promoter, space, status: 'bogus' })).rejects.toThrow(/Unknown/);
			await expect(t.bookings.message({ actor: await t.viewer(MOD), space, text: 'peek' })).rejects.toBeInstanceOf(
				ForbiddenError
			);
			const { thread, events } = await t.bookings.thread(promoter, space);
			expect(thread).toMatchObject({ status: 'confirmed', title: 'Night', project: { slug: 'bookable' } });
			expect(events.map((e) => e.kind)).toEqual(['inquiry', 'message', 'status']);
			expect((await t.bookings.threadsFor(DJ)).length).toBe(1);
			expect(await t.bookings.bySkey(thread.skey)).toBe(space);
			const auditDetails = (await t.audit.list({ action: 'booking.message' }))[0].details;
			expect(auditDetails).not.toContain('Yes');
		});

		it('keeps a personal planner and refuses accounts without spaces', async () => {
			const project = await t.projectAdmin.create({ actor: await t.viewer(ADMIN), profile: profile('Saved') });
			const promoter = await t.viewer(PROMOTER);
			await t.planner.save(promoter, project.uri, 'note');
			await t.planner.save(promoter, project.uri);
			expect(await t.planner.saves(promoter)).toHaveLength(1);

			const rkey = await t.planner.saveShow(promoter, {
				details: { title: 'Show' },
				lineup: [{ project: project.uri, status: 'confirmed' }]
			});

			await t.planner.saveShow(promoter, { rkey, details: { title: 'Show v2' }, lineup: [] });
			expect((await t.planner.shows(promoter)).map((s) => s.details.title)).toEqual(['Show v2']);
			await t.planner.deleteShow(promoter, rkey);
			await t.planner.unsave(promoter, project.uri);
			expect(await t.planner.saves(promoter)).toEqual([]);
			await expect(t.planner.saves(await t.viewer(LEGACY))).rejects.toThrow(/spaces/);
		});

		it('creates the planner space when a fresh account loads saves and shows at once', async () => {
			const promoter = await t.viewer(PROMOTER);

			// The planner page loads both in parallel, so both find no space and both try to create it.
			expect(await Promise.all([t.planner.saves(promoter), t.planner.shows(promoter)])).toEqual([[], []]);
			expect(t.net.spaces.has(plannerSpace(PROMOTER))).toBe(true);
		});

		it('surfaces a failed planner space creation and tries again on the next load', async () => {
			const promoter = await t.viewer(PROMOTER);
			t.net.failWrites(PROMOTER);

			await expect(t.planner.saves(promoter)).rejects.toThrow('UpstreamFailure');
			expect(await t.planner.saves(promoter)).toEqual([]);
		});

		it('skips planner records that fail their lexicon', async () => {
			const project = await t.projectAdmin.create({ actor: await t.viewer(ADMIN), profile: profile('Kept') });
			const promoter = await t.viewer(PROMOTER);
			await t.planner.save(promoter, project.uri);

			await t.net.client(PROMOTER).createSpaceRecord({
				space: plannerSpace(PROMOTER),
				collection: COLLECTION.plannerSave,
				record: { $type: COLLECTION.plannerSave, project: 42 }
			});

			expect((await t.planner.saves(promoter)).map((save) => save.project)).toEqual([project.uri]);
		});
	});
});
