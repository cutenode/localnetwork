import { XRPCError } from '@atproto/api';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { CALENDAR_EVENT, COLLECTION } from '#lib/nsid.ts';
import { EVENT_STATUS } from '#lib/shows.ts';
import type { ProjectView } from '#lib/types.ts';
import { ForbiddenError, NotFoundError, ReauthError, UserError } from '../core';
import { HappyViewClient } from '../happyview/client';
import { createTestServices } from '../testing/core';
import { FollowService } from './follows';
import { eventRecord, type ShowInput, ShowService } from './shows';

const ADMIN = 'did:plc:admin';

const DJ = 'did:plc:dj';

const FAN = 'did:plc:fan';

/** An account whose session can't write to its repo (signed out since, or a PDS that lost it). */
const LAPSED = 'did:plc:lapsed';

const BERLIN = { latE6: 52_520_000, lngE6: 13_405_000 };

/** Next year, so the shows stay upcoming. Berlin is on UTC+1 in November. */
const YEAR = new Date().getUTCFullYear() + 1;

const missingScope = () =>
	new XRPCError(403, 'ScopeMissingError', 'Missing required scope "repo:computer.localnetwork.follow"');

const input = (overrides: Partial<ShowInput> = {}): ShowInput => ({
	name: 'Tidal Hours',
	date: `${YEAR}-11-07`,
	start: '23:00',
	end: '06:00',
	venue: 'Halle Nord',
	location: { city: 'Berlin', country: 'DE', ...BERLIN },
	tickets: 'https://tickets.example/tidal',
	cancelled: false,
	...overrides
});

describe('shows and follows', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;
	let project: ProjectView;

	beforeEach(async () => {
		t = await createTestServices({ admins: [ADMIN], capable: [ADMIN, DJ, FAN] });
		const admin = await t.viewer(ADMIN);

		const created = await t.projectAdmin.create({
			actor: admin,
			profile: { name: 'Neon Tide', kind: 'solo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } }
		});

		project = await t.projectAdmin.setManagers({ actor: admin, project: created, managers: [DJ, LAPSED] });
	});
	afterEach(async () => {
		await t.audit.idle();
		await t.db.destroy();
	});

	const record = (did: string, collection: string, uri: string) =>
		t.net.records.get(`${did}|${collection}|${uri.split('/').at(-1)}`)?.value;

	/** A directory with HappyView's admin API, which records the backfills asked for. */
	const adminDirectory = () => {
		const backfills: unknown[] = [];

		const directory = new HappyViewClient({
			url: 'http://happyview.test',
			apiKey: 'key',
			fetch: async (resource, init) => {
				const request = new Request(resource, init);

				if (new URL(request.url).pathname !== '/admin/backfill') return t.happyView.fetch(request);
				backfills.push(await request.json());

				return Response.json({ id: 'job' });
			}
		});

		return { directory, backfills };
	};

	describe('FollowService', () => {
		it("follows from the follower's own repo once, counts it, and unfollows by deleting it", async () => {
			const fan = await t.viewer(FAN);

			await t.follows.follow(fan, project);
			await t.follows.follow(fan, project);
			const follows = [...t.net.records.values()].filter((stored) => stored.uri.includes(COLLECTION.follow));
			expect(follows.map((stored) => [stored.uri.split('/')[2], stored.value.subject])).toEqual([[FAN, project.uri]]);
			expect(await t.follows.state(project, FAN)).toEqual({ followers: 1, following: true });
			expect(await t.follows.state(project, undefined)).toEqual({ followers: 1, following: false });

			await t.follows.unfollow(fan, project);
			await t.follows.unfollow(fan, project);
			expect(t.net.records.has(follows[0].uri.split('/').slice(2).join('|'))).toBe(false);
			expect(await t.follows.state(project, FAN)).toEqual({ followers: 0, following: false });
			expect((await t.audit.list({ action: 'follow' })).map((entry) => entry.action)).toEqual([
				'follow.delete',
				'follow.create'
			]);
		});

		it('deletes every follow an account has of a project when it unfollows', async () => {
			const fan = await t.viewer(FAN);
			const client = t.net.client(FAN);
			const follow = { $type: COLLECTION.follow, subject: project.uri, createdAt: '2026-10-01T00:00:00.000Z' };
			await client.createRecord({ collection: COLLECTION.follow, record: follow });
			await client.createRecord({ collection: COLLECTION.follow, record: follow });
			expect(await t.follows.state(project, FAN)).toEqual({ followers: 1, following: true });

			await t.follows.unfollow(fan, project);
			expect([...t.net.records.keys()].filter((key) => key.startsWith(`${FAN}|${COLLECTION.follow}|`))).toEqual([]);
			expect(await t.follows.state(project, FAN)).toEqual({ followers: 0, following: false });
		});

		it("says following isn't available while the directory is down", async () => {
			const down = new HappyViewClient({
				url: 'http://happyview.test',
				fetch: () => Promise.reject(new Error('down'))
			});

			const follows = new FollowService(t.core, down, 0);
			const fan = await t.viewer(FAN);

			for (const change of [() => follows.follow(fan, project), () => follows.unfollow(fan, project)])
				await expect(change()).rejects.toMatchObject({
					status: 503,
					message: "Following isn't available right now. Try again in a minute."
				});
		});

		it("has HappyView re-read the follower's follows when it doesn't serve a new one in time", async () => {
			const { directory, backfills } = adminDirectory();
			// HappyView never hears of the write, as when Jetstream drops it.
			t.net.onRecord = async () => {};

			await new FollowService(t.core, directory, 0).follow(await t.viewer(FAN), project);
			expect(backfills).toEqual([{ collection: COLLECTION.follow, did: FAN }]);
		});

		it("says the account's server can't, rather than asking again, when a fresh sign-in didn't get the scope", async () => {
			await t.accounts.recordLogin({
				did: FAN,
				handle: null,
				pdsUrl: null,
				grantedScope: 'atproto',
				spacesCapable: false,
				publicWrites: false
			});
			t.net.failWrites(FAN, 1, missingScope());
			const failed = t.follows.follow(await t.viewer(FAN), project);
			await expect(failed).rejects.toThrow("Your account's server doesn't allow this yet");
			await expect(failed).rejects.not.toBeInstanceOf(ReauthError);
		});

		it('asks to sign in again when the session is gone or lacks the scope, and passes other failures on', async () => {
			const fan = await t.viewer(FAN);

			await expect(t.follows.follow(await t.viewer(LAPSED), project)).rejects.toThrow(ReauthError);
			t.net.failWrites(FAN, 1, missingScope());
			await expect(t.follows.follow(fan, project)).rejects.toMatchObject({
				status: 401,
				message: 'Sign in again to follow DJs'
			});
			t.net.failWrites(FAN);
			await expect(t.follows.follow(fan, project)).rejects.toBeInstanceOf(XRPCError);
			expect(await t.follows.state(project, FAN)).toEqual({ followers: 0, following: false });
		});

		it("doesn't follow a project that isn't listed", async () => {
			const hidden = await t.projectAdmin.setStatus({ actor: await t.viewer(ADMIN), project, status: 'hidden' });

			await expect(t.follows.follow(await t.viewer(FAN), hidden)).rejects.toBeInstanceOf(NotFoundError);
		});
	});

	describe('ShowService', () => {
		it("announces a calendar event in the manager's repo, and a show copying it that listings find", async () => {
			const ref = await t.shows.save(await t.viewer(DJ), project, input());

			const event = record(DJ, CALENDAR_EVENT, ref.uri);
			expect(event).toEqual({
				$type: CALENDAR_EVENT,
				name: 'Tidal Hours',
				startsAt: `${YEAR}-11-07T23:00:00+01:00`,
				endsAt: `${YEAR}-11-08T06:00:00+01:00`,
				mode: 'community.lexicon.calendar.event#inperson',
				status: EVENT_STATUS.scheduled,
				locations: [
					{ $type: 'community.lexicon.location.address', locality: 'Berlin', country: 'DE', name: 'Halle Nord' },
					{ $type: 'community.lexicon.location.geo', latitude: '52.52', longitude: '13.405', name: 'Halle Nord' }
				],
				uris: [{ uri: 'https://tickets.example/tidal', name: 'Tickets' }],
				createdAt: expect.any(String)
			});

			const eventRef = t.net.records.get(`${DJ}|${CALENDAR_EVENT}|${ref.uri.split('/').at(-1)}`)!;
			expect(record(DJ, COLLECTION.show, ref.uri)).toMatchObject({
				project: project.uri,
				event: { uri: eventRef.uri, cid: eventRef.cid },
				name: 'Tidal Hours',
				startsAt: event?.startsAt,
				endsAt: event?.endsAt,
				venue: 'Halle Nord',
				location: { city: 'Berlin', country: 'DE', ...BERLIN },
				status: EVENT_STATUS.scheduled,
				tickets: 'https://tickets.example/tidal'
			});

			const [listed] = await t.shows.forProject(project);
			expect(listed).toMatchObject({ uri: ref.uri, did: DJ, project: { uri: project.uri } });
			expect(
				(await t.shows.upcoming({ near: { lat: 52.5, lng: 13.4 }, radiusKm: 25 })).items[0].distanceKm
			).toBeLessThan(5);
			expect((await t.shows.upcoming({ follower: FAN })).items).toEqual([]);
			await t.follows.follow(await t.viewer(FAN), project);
			expect((await t.shows.upcoming({ follower: FAN })).items.map((show) => show.uri)).toEqual([ref.uri]);
			expect((await t.audit.list({ action: 'show.save' }))[0].summary).toBe(
				'Announced the show “Tidal Hours” for Neon Tide'
			);
		});

		it('writes the event and the show together, so a failed write leaves neither', async () => {
			t.net.failWrites(DJ);
			await expect(t.shows.save(await t.viewer(DJ), project, input())).rejects.toBeInstanceOf(XRPCError);
			expect([...t.net.records.keys()].filter((key) => key.startsWith(`${DJ}|`))).toEqual([]);
		});

		it("checks the show against its lexicon before writing the event, so a show that doesn't fit writes nothing", async () => {
			// The event's name has no limit; the show's copy of it does.
			const long = input({ name: 'x'.repeat(121) });

			await expect(t.shows.save(await t.viewer(DJ), project, long)).rejects.toThrow(
				/Invalid computer.localnetwork.show/
			);
			expect([...t.net.records.keys()].filter((key) => key.startsWith(`${DJ}|`))).toEqual([]);
		});

		it("has HappyView re-read the manager's shows when it doesn't serve a write in time", async () => {
			const { directory, backfills } = adminDirectory();
			t.net.onRecord = async () => {};

			await new ShowService(t.core, t.projects, directory, t.publicRepo, 0).save(await t.viewer(DJ), project, input());
			expect(backfills).toEqual([{ collection: COLLECTION.show, did: DJ }]);
		});

		it("has HappyView re-read each manager's shows, one account at a time", async () => {
			const { directory, backfills } = adminDirectory();

			await new ShowService(t.core, t.projects, directory, t.publicRepo, 0).reindexManagers();
			expect(backfills).toEqual([DJ, LAPSED].map((did) => ({ collection: COLLECTION.show, did })));
		});

		it('lets only managers announce, for listed projects', async () => {
			await expect(t.shows.save(await t.viewer(FAN), project, input())).rejects.toBeInstanceOf(ForbiddenError);
			const hidden = await t.projectAdmin.setStatus({ actor: await t.viewer(ADMIN), project, status: 'hidden' });
			await expect(t.shows.save(await t.viewer(DJ), hidden, input())).rejects.toBeInstanceOf(UserError);
			await expect(t.shows.save(await t.viewer(LAPSED), project, input())).rejects.toMatchObject({
				status: 401,
				message: 'Sign in again to announce shows'
			});
		});

		it('spots an event another app changed, and re-copies it when edited, keeping what the other app added', async () => {
			const dj = await t.viewer(DJ);
			const ref = await t.shows.save(dj, project, input());
			const rkey = ref.uri.split('/').at(-1)!;
			const event = record(DJ, CALENDAR_EVENT, ref.uri)!;

			await t.net.client(DJ).putRecord({
				collection: CALENDAR_EVENT,
				rkey,
				record: { ...event, name: 'Tidal Hours (sold out)', description: 'Doors at 22:00' }
			});

			const shows = await t.shows.forProject(project);
			expect(await t.shows.edited(DJ, shows)).toEqual([ref.uri]);
			expect(await t.shows.edited(FAN, shows)).toEqual([]);
			expect(await t.shows.values(dj, project, rkey)).toMatchObject({
				name: 'Tidal Hours (sold out)',
				date: `${YEAR}-11-07`,
				start: '23:00',
				end: '06:00',
				venue: 'Halle Nord',
				city: 'Berlin',
				latE6: String(BERLIN.latE6),
				tickets: 'https://tickets.example/tidal',
				cancelled: false
			});

			await t.shows.save(dj, project, input({ name: 'Tidal Hours (sold out)', start: '22:00', end: undefined }), rkey);
			const moved = record(DJ, CALENDAR_EVENT, ref.uri)!;
			expect(moved).toMatchObject({ startsAt: `${YEAR}-11-07T22:00:00+01:00`, status: EVENT_STATUS.rescheduled });
			expect(moved).toMatchObject({ description: 'Doors at 22:00', createdAt: event.createdAt });
			expect(moved.endsAt).toBeUndefined();
			expect(await t.shows.edited(DJ, await t.shows.forProject(project))).toEqual([]);
			expect((await t.shows.forProject(project))[0].record).toMatchObject({
				name: 'Tidal Hours (sold out)',
				status: EVENT_STATUS.rescheduled
			});

			// A show that moved stays marked as moved through later edits.
			await t.shows.save(dj, project, input({ name: 'Tidal Hours', start: '22:00', end: undefined }), rkey);
			expect(record(DJ, CALENDAR_EVENT, ref.uri)).toMatchObject({
				name: 'Tidal Hours',
				status: EVENT_STATUS.rescheduled
			});

			await t.shows.save(dj, project, input({ start: '22:00', cancelled: true }), rkey);
			expect((await t.shows.forProject(project))[0].record.status).toBe(EVENT_STATUS.cancelled);
			expect(await t.shows.values(dj, project, rkey)).toMatchObject({ cancelled: true, end: '06:00' });
		});

		it('deletes a show and its event, and finds only shows of the project asked about', async () => {
			const dj = await t.viewer(DJ);
			const ref = await t.shows.save(dj, project, input());
			const rkey = ref.uri.split('/').at(-1)!;

			const other = await t.projectAdmin.create({
				actor: await t.viewer(ADMIN),
				profile: { name: 'Other', kind: 'solo', genres: ['house'], location: { city: 'Berlin', country: 'DE' } }
			});

			expect(await t.shows.values(dj, other, rkey)).toBeNull();
			await expect(t.shows.save(dj, project, input(), 'missing')).rejects.toBeInstanceOf(NotFoundError);
			await expect(t.shows.remove(dj, other, rkey)).rejects.toBeInstanceOf(NotFoundError);

			await t.shows.remove(dj, project, rkey);
			expect(record(DJ, COLLECTION.show, ref.uri)).toBeUndefined();
			expect(record(DJ, CALENDAR_EVENT, ref.uri)).toBeUndefined();
			expect(await t.shows.forProject(project)).toEqual([]);
			expect(await t.shows.values(dj, project, rkey)).toBeNull();
			expect((await t.audit.list({ action: 'show.delete' }))[0].summary).toBe(
				'Deleted the show “Tidal Hours” for Neon Tide'
			);
		});
	});

	describe('eventRecord', () => {
		const listed = (startsAt: string, status: string = EVENT_STATUS.scheduled) => ({
			project: 'at://did:plc:root/computer.localnetwork.project/p',
			event: { uri: 'at://did:plc:dj/community.lexicon.calendar.event/e', cid: 'c' },
			name: 'Tidal Hours',
			startsAt,
			location: { city: 'Berlin', country: 'DE', ...BERLIN },
			status,
			createdAt: '2026-10-01T00:00:00.000Z'
		});

		it('marks a show rescheduled when its start moves, comparing instants, and keeps it so', () => {
			// 23:00 in Berlin is 22:00 UTC, however it's written.
			expect(eventRecord(input(), null, listed(`${YEAR}-11-07T22:00:00.000Z`)).status).toBe(EVENT_STATUS.scheduled);
			expect(eventRecord(input(), null, listed(`${YEAR}-11-07T22:00:00Z`)).status).toBe(EVENT_STATUS.scheduled);
			expect(eventRecord(input({ start: '22:00' }), null, listed(`${YEAR}-11-07T22:00:00Z`)).status).toBe(
				EVENT_STATUS.rescheduled
			);
			const moved = listed(`${YEAR}-11-07T23:00:00+01:00`, EVENT_STATUS.rescheduled);
			expect(eventRecord(input(), null, moved).status).toBe(EVENT_STATUS.rescheduled);
			expect(eventRecord(input()).status).toBe(EVENT_STATUS.scheduled);
		});
	});
});
