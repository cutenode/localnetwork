import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { HappyViewError } from '#lib/server/happyview/client.ts';
import { silentLogger } from '#lib/server/logger.ts';
import { ADMIN, DJ, pressKitServices, PROMOTER, STRANGER } from '#lib/server/testing/press-kits.ts';
import { writtenLog } from '#lib/server/testing/written-log.ts';
import type { BookingThreadView, PlannerShowView, RequestView, ShowView } from '#lib/types.ts';
import { accessWaiting, needsCount, needsYou, nextNights, soon, withOthers, type NightWindow } from './home';

let f: Awaited<ReturnType<typeof pressKitServices>>;

const NOW = Date.parse('2026-11-06T12:00:00Z');

beforeEach(async () => {
	f = await pressKitServices();
});

afterEach(async () => {
	await f.t.audit.idle();
	await f.t.db.destroy();
});

const inquire = async (title: string, date?: string) =>
	f.t.bookings.start({
		actor: await f.t.viewer(PROMOTER),
		projectUri: f.project.uri,
		show: { title, date: date && `${date}T00:00:00.000Z` },
		message: 'Free?'
	});

describe('needsYou', () => {
	it('lists replies by night, undated ones next, and ones whose date went by last', async () => {
		await inquire('Passed', '2026-10-01');
		await inquire('Undated');
		await inquire('Later', '2026-12-12');
		await inquire('Sooner', '2026-11-14');

		const rows = await needsYou(f.t, await f.t.viewer(DJ), NOW);

		expect(rows.map((row) => row.title)).toEqual([
			'Reply to Sooner',
			'Reply to Later',
			'Reply to Undated',
			'Reply to Passed'
		]);
		expect(rows[0]).toMatchObject({ scope: 'Neon Tide', action: 'Reply' });
		expect(rows[0].detail).toMatch(/^Sat 14 Nov · New inquiry from /);
		expect(rows[2].detail).toMatch(/^No date yet · /);
		expect(rows[3].detail).toMatch(/^Thu 1 Oct, date passed · /);
	});

	it('keeps undated replies newest first, and dates a sent-back request by its last change without a decision time', async () => {
		await inquire('First undated');
		await inquire('Second undated');

		const request: RequestView = {
			uri: 'at://request/1',
			cid: 'c',
			rkey: '1',
			kind: 'create',
			status: 'changes_requested',
			requester: { did: DJ, handle: null },
			repoDid: DJ,
			projectUri: null,
			project: null,
			record: {
				kind: 'create',
				profile: { ...f.project.profile, name: 'Third Act' },
				createdAt: '2026-11-01T00:00:00.000Z'
			},
			createdAt: '2026-11-01T00:00:00.000Z',
			updatedAt: '2026-11-05T12:00:00.000Z',
			decision: null,
			resultProjectSlug: null,
			deletedAt: null
		};

		const requests = { list: async () => [request] };

		const rows = await needsYou({ bookings: f.t.bookings, requests }, await f.t.viewer(DJ), NOW);

		expect(rows.map((row) => row.title)).toEqual([
			'Reply to Second undated',
			'Reply to First undated',
			'Update your request'
		]);
		expect(rows[2].detail).toMatch(/ · yesterday$/);
	});

	it('words a reply for the promoter by the act, and leaves threads out once they close or wait on the other side', async () => {
		const answered = await inquire('Answered', '2026-11-14');
		const declined = await inquire('Declined', '2026-11-15');
		await inquire('Unanswered', '2026-11-16');
		const dj = await f.t.viewer(DJ);
		await f.t.bookings.message({ actor: dj, space: answered, text: 'Yes' });
		await f.t.bookings.setStatus({ actor: dj, space: declined, status: 'declined' });

		const rows = await needsYou(f.t, await f.t.viewer(PROMOTER), NOW);

		expect(rows.map((row) => row.title)).toEqual(['Answer Neon Tide about Answered']);
		expect(rows[0].detail).not.toContain('New inquiry');
		expect((await needsYou(f.t, dj, NOW)).map((row) => row.title)).toEqual(['Reply to Unanswered']);
	});

	it('adds requests staff sent back, after the bookings', async () => {
		await inquire('Night', '2026-11-14');
		const dj = await f.t.viewer(DJ);

		const request = await f.t.requests.submit({
			actor: dj,
			kind: 'create',
			profile: { ...f.project.profile, name: 'Second Act' }
		});

		await f.t.requests.decide({
			actor: await f.t.viewer(ADMIN),
			uri: request.uri,
			outcome: 'changesRequested',
			reason: 'Link your socials'
		});

		const rows = await needsYou(f.t, dj, NOW);

		expect(rows.map((row) => row.title)).toEqual(['Reply to Night', 'Update your request']);
		expect(rows[1]).toMatchObject({ action: 'Update', href: `/dashboard/account/requests/${request.rkey}` });
		expect(rows[1].detail).toMatch(/^New project: Second Act · Staff asked for changes · /);
	});
});

describe("a thread the directory can't name", () => {
	const thread: BookingThreadView = {
		space: 'space',
		skey: '3k',
		status: 'negotiating',
		title: null,
		showDate: '2026-11-20T00:00:00.000Z',
		project: null,
		promoter: { did: PROMOTER, handle: 'pat.test' },
		participants: [
			{ did: PROMOTER, handle: 'pat.test', role: 'promoter' },
			{ did: DJ, handle: 'neon.test', role: 'manager' }
		],
		lastAuthor: DJ,
		lastActivityAt: '2026-11-05T00:00:00.000Z',
		createdAt: '2026-11-01T00:00:00.000Z'
	};

	it('still says what to do, without an act to name', async () => {
		const app = { bookings: { threadsFor: async () => [thread] }, requests: { list: async () => [] } };

		const [row] = await needsYou(app, await f.t.viewer(PROMOTER), NOW);

		expect(row).toMatchObject({ title: 'Reply to a booking', scope: 'a booking' });
	});

	it('still lists the night', async () => {
		const app = {
			shows: { forProject: async () => [] },
			planner: { saves: async () => [], shows: async () => [] },
			projects: { byUris: async () => new Map() }
		};

		const { nights } = await nextNights(
			{ app, log: silentLogger },
			await f.t.viewer(PROMOTER),
			[thread],
			null,
			{ promoter: false },
			{},
			NOW
		);

		expect(nights).toMatchObject([{ title: 'Booking', detail: 'Booking · not confirmed', tentative: true }]);
	});
});

describe('needsCount', () => {
	it('counts what needsYou lists, from the index alone', async () => {
		const closed = await inquire('Closed', '2026-11-14');
		await inquire('Open', '2026-11-15');
		const dj = await f.t.viewer(DJ);
		await f.t.bookings.setStatus({ actor: dj, space: closed, status: 'declined' });

		const request = await f.t.requests.submit({
			actor: dj,
			kind: 'create',
			profile: { ...f.project.profile, name: 'Second Act' }
		});

		await f.t.requests.submit({ actor: dj, kind: 'create', profile: { ...f.project.profile, name: 'Third Act' } });
		await f.t.requests.decide({
			actor: await f.t.viewer(ADMIN),
			uri: request.uri,
			outcome: 'changesRequested',
			reason: 'Link your socials'
		});

		expect(await needsCount(f.t, DJ)).toBe(2);
		expect(await needsYou(f.t, dj, NOW)).toHaveLength(2);
		expect(await needsCount(f.t, STRANGER)).toBe(0);
	});
});

describe('soon', () => {
	it('hands back a result that is ready, and the promise itself for one that is not', async () => {
		expect((await soon(Promise.resolve('ready'))).value).toBe('ready');

		const slow = new Promise(() => {});
		expect((await soon(slow)).value).toBe(slow);
	});

	it('leaves a failure to stream, for the page to show', async () => {
		const failed = Promise.reject(new Error('down'));

		await expect((await soon(failed)).value).rejects.toThrow('down');
	});
});

describe('needsCount', () => {
	it('counts what needsYou lists, from the index alone', async () => {
		const closed = await inquire('Closed', '2026-11-14');
		await inquire('Open', '2026-11-15');
		const dj = await f.t.viewer(DJ);
		await f.t.bookings.setStatus({ actor: dj, space: closed, status: 'declined' });

		const request = await f.t.requests.submit({
			actor: dj,
			kind: 'create',
			profile: { ...f.project.profile, name: 'Second Act' }
		});

		await f.t.requests.submit({ actor: dj, kind: 'create', profile: { ...f.project.profile, name: 'Third Act' } });
		await f.t.requests.decide({
			actor: await f.t.viewer(ADMIN),
			uri: request.uri,
			outcome: 'changesRequested',
			reason: 'Link your socials'
		});

		expect(await needsCount(f.t, DJ)).toBe(2);
		expect(await needsYou(f.t, dj, NOW)).toHaveLength(2);
		expect(await needsCount(f.t, STRANGER)).toBe(0);
	});
});

describe('soon', () => {
	it('hands back a result that is ready, and the promise itself for one that is not', async () => {
		expect((await soon(Promise.resolve('ready'))).value).toBe('ready');

		const slow = new Promise(() => {});
		expect((await soon(slow)).value).toBe(slow);
	});

	it('leaves a failure to stream, for the page to show', async () => {
		const failed = Promise.reject(new Error('down'));

		await expect((await soon(failed)).value).rejects.toThrow('down');
	});
});

describe('accessWaiting', () => {
	it("asks about each person waiting on a press kit, and drops them once they're answered", async () => {
		const epk = await f.publish(DJ);
		await f.t.epks.request(await f.t.viewer(STRANGER), f.kit(epk), { role: 'press' });

		const rows = await accessWaiting({ app: f.t, log: silentLogger }, [f.project], NOW);

		expect(rows).toHaveLength(1);
		expect(rows[0]).toMatchObject({
			scope: 'Neon Tide',
			action: 'Review',
			href: `/dashboard/projects/neon-tide/press-kits/${epk.rkey}/access`
		});
		expect(rows[0].title).toMatch(/^Let .+ see Clubs\?$/);

		const [request] = await f.t.epks.requests(epk);
		await f.t.epks.decide(await f.t.viewer(DJ), f.kit(epk), request.uri, 'approve');

		expect(await accessWaiting({ app: f.t, log: silentLogger }, [f.project], NOW)).toEqual([]);
	});

	it("leaves them out, and says so in the log, when the press kits can't be read", async () => {
		const { logger: log, notes } = writtenLog('info');
		const epks = { waiting: () => Promise.reject(new Error('directory down')) };

		expect(await accessWaiting({ app: { epks }, log }, [f.project], NOW)).toEqual([]);
		expect(notes()).toContainEqual(expect.objectContaining({ level: 'warn', err: 'directory down' }));
	});
});

describe('withOthers', () => {
	const thread = (role: 'promoter' | 'manager'): BookingThreadView => ({
		space: role,
		skey: role,
		status: 'open',
		title: 'Night',
		showDate: null,
		project: null,
		promoter: { did: 'did:plc:other', handle: null },
		participants: [{ did: 'did:plc:me', handle: null, role }],
		lastAuthor: 'did:plc:me',
		lastActivityAt: '2026-11-01T00:00:00Z',
		createdAt: '2026-11-01T00:00:00Z'
	});

	// SAFETY: withOthers only counts requests.
	const request = {} as RequestView;

	it('says what is with whom, and nothing with nothing out', () => {
		expect(withOthers([], [], 'did:plc:me')).toBeNull();
		expect(withOthers([thread('manager')], [], 'did:plc:me')).toBe(
			'1 booking is waiting on a promoter. Nothing needed from you.'
		);
		expect(withOthers([thread('promoter'), thread('promoter')], [request], 'did:plc:me')).toBe(
			'2 bookings are waiting on DJs and 1 request is with staff. Nothing needed from you.'
		);
		expect(
			withOthers([thread('manager'), thread('manager'), thread('promoter')], [request, request], 'did:plc:me')
		).toBe(
			'2 bookings are waiting on promoters and 1 booking is waiting on a DJ and 2 requests are with staff. Nothing needed from you.'
		);
	});
});

describe('nextNights', () => {
	// SAFETY: the rows read only these fields of a show and its project.
	const announced = (name: string, startsAt: string, venue?: string) =>
		({
			uri: `at://did:plc:dj/show/${name}`,
			rkey: name,
			record: { name, startsAt, venue },
			project: f.project
		}) as ShowView;

	const planned = (title: string, date?: string): PlannerShowView => ({
		rkey: title,
		details: { title, date, venue: 'Kantine' },
		lineup: [{ project: f.project.uri }],
		createdAt: '2026-10-01T00:00:00Z'
	});

	const open = async ({
		shows = [],
		plans = [],
		threads = [],
		sections = { promoter: false },
		window = {},
		as = DJ
	}: {
		shows?: ShowView[] | Error;
		plans?: PlannerShowView[] | Error;
		threads?: BookingThreadView[];
		sections?: { promoter: boolean } | null;
		window?: NightWindow;
		as?: string;
	}) => {
		const answer = async <T>(result: T | Error) => {
			if (result instanceof Error) throw result;

			return result;
		};

		const app = {
			shows: { forProject: () => answer(shows) },
			planner: { saves: async () => [], shows: () => answer(plans) },
			projects: { byUris: async () => new Map() }
		};

		return nextNights({ app, log: silentLogger }, await f.t.viewer(as), threads, [f.project], sections, window, NOW);
	};

	it('puts shows, bookings and plans in one list by date, without what already went by', async () => {
		await inquire('Rooftop', '2026-11-20');
		await inquire('Undated');
		await inquire('Over', '2026-10-01');

		const { nights, more, failed } = await open({
			shows: [
				announced('Closing', '2026-11-07T23:30:00.000Z', 'Tiefgarage'),
				announced('Old', '2026-11-05T23:30:00.000Z')
			],
			plans: [planned('Winter', '2026-11-06T22:00:00.000Z'), planned('Someday')],
			threads: await f.t.bookings.threadsFor(DJ),
			sections: { promoter: true }
		});

		expect(nights.map(({ title, when, tentative }) => [title, when, tentative])).toEqual([
			['Winter', 'Tonight', true],
			['Closing', 'Tomorrow', false],
			['Rooftop', undefined, true]
		]);
		expect(nights[1].detail).toBe('You play 23:30 as Neon Tide · Tiefgarage');
		expect(nights[0].detail).toBe('Planned · Kantine · 1 DJ');
		expect(nights[2].detail).toBe('Booking for Neon Tide · not confirmed');
		expect(more).toBe(0);
		expect(failed).toBe(false);
	});

	it('shows a booking that became a show once, as the show', async () => {
		await inquire('Rooftop', '2026-11-20');

		const { nights } = await open({
			shows: [announced('Rooftop', '2026-11-20T22:00:00.000Z')],
			threads: await f.t.bookings.threadsFor(DJ)
		});

		expect(nights).toHaveLength(1);
		expect(nights[0].tentative).toBe(false);
	});

	it('marks a confirmed booking settled, and stops at five', async () => {
		const space = await inquire('Booked', '2026-11-08');
		await f.t.bookings.setStatus({ actor: await f.t.viewer(DJ), space, status: 'confirmed' });

		const { nights, more } = await open({
			shows: [1, 2, 3, 4, 5, 6].map((day) => announced(`Night ${day}`, `2026-12-0${day}T22:00:00.000Z`)),
			threads: await f.t.bookings.threadsFor(DJ)
		});

		expect(nights[0]).toMatchObject({ title: 'Booked', tentative: false, detail: 'Booking for Neon Tide' });
		expect(nights).toHaveLength(5);
		expect(more).toBe(2);
	});

	it('leaves the lineup out of a plan with nobody on it yet', async () => {
		const { nights } = await open({
			plans: [{ ...planned('Winter', '2026-11-20T22:00:00.000Z'), lineup: [] }],
			sections: { promoter: true }
		});

		expect(nights[0].detail).toBe('Planned · Kantine');
	});

	it('lists every night up to a horizon when asked to', async () => {
		const shows = ['2026-11-20', '2026-12-01', '2027-03-01', '2027-12-01'].map((day, at) =>
			announced(`Night ${at}`, `${day}T22:00:00.000Z`)
		);

		const { nights, more } = await open({ shows, window: { shown: Infinity, ahead: 365 } });

		expect(nights.map((night) => night.title)).toEqual(['Night 0', 'Night 1', 'Night 2']);
		expect(more).toBe(0);
	});

	it("counts an act's unconfirmed bookings that share a night on the first of them, but not a promoter's", async () => {
		await inquire('Rooftop', '2026-11-20');
		await inquire('Basement', '2026-11-20');
		await inquire('Alone', '2026-11-21');
		const settled = await inquire('Booked', '2026-11-21');
		await f.t.bookings.setStatus({ actor: await f.t.viewer(DJ), space: settled, status: 'confirmed' });

		const { nights } = await open({ threads: await f.t.bookings.threadsFor(DJ) });

		expect(nights.map(({ date, inquiries }) => [date, inquiries])).toEqual([
			['2026-11-20', 2],
			['2026-11-20', undefined],
			['2026-11-21', undefined],
			['2026-11-21', undefined]
		]);

		const promoter = await open({ threads: await f.t.bookings.threadsFor(PROMOTER), as: PROMOTER });

		expect(promoter.nights.map((night) => night.inquiries)).toEqual([undefined, undefined, undefined, undefined]);
	});

	it("leaves a performer's planner unread, but keeps it for someone whose sections aren't known", async () => {
		const plans = [planned('Winter', '2026-11-20T22:00:00.000Z')];

		expect((await open({ plans })).nights).toEqual([]);
		expect((await open({ plans, sections: null })).nights).toHaveLength(1);
	});

	it('says some are missing when the directory or the planner fails, and keeps the rest', async () => {
		await inquire('Rooftop', '2026-11-20');
		const threads = await f.t.bookings.threadsFor(DJ);

		const offline = await open({ shows: new HappyViewError(503, 'down'), threads });

		expect(offline.failed).toBe(true);
		expect(offline.nights.map((night) => night.title)).toEqual(['Rooftop']);

		const planner = await open({ plans: new Error('no answer'), threads, sections: { promoter: true } });

		expect(planner.failed).toBe(true);
		expect(planner.nights.map((night) => night.title)).toEqual(['Rooftop']);
	});
});
