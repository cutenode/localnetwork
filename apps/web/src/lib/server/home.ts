import { setTimeout as sleep } from 'node:timers/promises';
import { bookingGroup } from '#lib/bookings.ts';
import { formatDate, nightLabel, pluralize, requestTitle, timeAgo, accountLabel } from '#lib/format.ts';
import type { NeedsRow, NightRow } from '#lib/home.ts';
import type { BookingThreadView, PlannerShowView, ProjectView, RequestView, Viewer } from '#lib/types.ts';
import { unlessOffline } from './happyview/offline';
import type { Logger } from './logger';
import { loadPlanner } from './planner-page';
import type { BookingService } from './services/bookings';
import type { EpkService } from './services/epk';
import type { PlannerService } from './services/planner';
import type { ProjectService } from './services/projects';
import type { RequestService } from './services/requests';
import type { ShowService } from './services/shows';

type Log = Pick<Logger, 'warn'>;

/** How many of an account's projects Home asks the directory about, so nobody's page waits on dozens of lookups. */
const PROJECTS_READ = 10;

const NIGHTS_SHOWN = 5;

/** How long a load holds the first paint for a slower source, so a healthy one renders with no placeholders at all. */
const SOON_MS = 150;

/**
 * `work` as a load should return it: its result if that's ready within {@link SOON_MS}, else the promise itself to
 * stream. A failure streams too, for the page to show where the result would have gone.
 */
export const soon = async <T>(work: Promise<T>): Promise<{ value: T | Promise<T> }> =>
	Promise.race([
		work.then(
			(value) => ({ value }),
			() => ({ value: work })
		),
		sleep(SOON_MS).then(() => ({ value: work }))
	]);

const dayOf = (iso: string) => iso.slice(0, 10);

const today = (now: number) => new Date(now).toISOString().slice(0, 10);

const roleIn = (thread: BookingThreadView, did: string) =>
	thread.participants.find((participant) => participant.did === did)?.role;

/** A thread waiting on `viewer`, with its night first when it has one. */
function bookingRow(thread: BookingThreadView, viewer: Viewer, now: number): NeedsRow {
	const title = thread.title ?? 'a booking';
	const act = thread.project?.name;
	const isManager = roleIn(thread, viewer.did) === 'manager';
	const isPast = !!thread.showDate && dayOf(thread.showDate) < today(now);
	const night = thread.showDate ? nightLabel(thread.showDate) : 'No date yet';
	const inquiry = thread.status === 'open' ? 'New inquiry from' : 'From';

	return {
		key: thread.skey,
		scope: act ?? title,
		title: isManager || !act ? `Reply to ${title}` : `Answer ${act} about ${title}`,
		detail: [
			isPast ? `${night}, date passed` : night,
			isManager && `${inquiry} ${accountLabel(thread.promoter)}`,
			timeAgo(thread.lastActivityAt, now)
		]
			.filter(Boolean)
			.join(' · '),
		href: `/dashboard/bookings/${thread.skey}`,
		action: 'Reply'
	};
}

/** Nights still to come first, soonest on top, then threads with no date, then ones whose date went by. */
function byNight(a: BookingThreadView, b: BookingThreadView, now: number) {
	const rank = ({ showDate }: BookingThreadView) => {
		if (!showDate) return 1;

		return dayOf(showDate) < today(now) ? 2 : 0;
	};

	return rank(a) - rank(b) || (a.showDate ?? '').localeCompare(b.showDate ?? '');
}

const requestRow = (request: RequestView, now: number): NeedsRow => ({
	key: request.uri,
	scope: requestTitle(request),
	title: 'Update your review request',
	detail: `${requestTitle(request)} · Staff asked for changes · ${timeAgo(request.decision?.at ?? request.updatedAt, now)}`,
	href: `/dashboard/requests/${request.rkey}`,
	action: 'Update'
});

/**
 * How many of {@link needsYou}'s rows there'll be. It reads only the site's index, so every dashboard page can say at
 * once, and Home can hold the rows' room, however slow the directory is.
 */
export async function needsCount(
	app: { bookings: Pick<BookingService, 'waitingOn'>; requests: Pick<RequestService, 'sentBack'> },
	did: string
): Promise<number> {
	const [replies, sentBack] = await Promise.all([app.bookings.waitingOn(did), app.requests.sentBack(did)]);

	return replies + sentBack;
}

/** What's waiting on `viewer` in the order Home shows it: bookings, then requests staff sent back. */
export async function needsYou(
	app: { bookings: Pick<BookingService, 'threadsFor'>; requests: Pick<RequestService, 'list'> },
	viewer: Viewer,
	now = Date.now()
): Promise<NeedsRow[]> {
	const [threads, requests] = await Promise.all([
		app.bookings.threadsFor(viewer.did),
		app.requests.list({ requester: viewer.did, status: 'changes_requested' })
	]);

	return [
		...threads
			.filter((thread) => bookingGroup(thread, viewer.did) === 'reply')
			.sort((a, b) => byNight(a, b, now))
			.map((thread) => bookingRow(thread, viewer, now)),
		...requests.map((request) => requestRow(request, now))
	];
}

/** People asking to see a press kit of one of `projects`. It takes the directory to know the kits, so this streams. */
export async function accessWaiting(
	{ app, log }: { app: { epks: Pick<EpkService, 'waiting'> }; log: Log },
	projects: ProjectView[],
	now = Date.now()
): Promise<NeedsRow[]> {
	try {
		const waiting = await Promise.all(projects.slice(0, PROJECTS_READ).map((project) => app.epks.waiting(project)));

		return waiting.flat().map(({ project, epk, requester, createdAt }): NeedsRow => ({
			key: `${epk.uri} ${requester.did}`,
			scope: project.profile.name,
			title: `Let ${accountLabel(requester)} see ${epk.record.title}?`,
			detail: `Asked ${timeAgo(createdAt, now)}`,
			href: `/dashboard/projects/${project.slug}/press-kits/${epk.rkey}/access`,
			action: 'Review'
		}));
	} catch (error) {
		log.warn('could not read who is waiting on a press kit', { err: error });

		return [];
	}
}

/** The line under Home's lists saying what's with someone else, or null with nothing out. */
export function withOthers(threads: BookingThreadView[], requests: RequestView[], did: string): string | null {
	const waiting = threads.filter((thread) => bookingGroup(thread, did) === 'active');
	const onPromoters = waiting.filter((thread) => roleIn(thread, did) === 'manager').length;
	const onDjs = waiting.length - onPromoters;

	const parts = [
		onPromoters &&
			`${pluralize(onPromoters, 'booking')} ${onPromoters === 1 ? 'is waiting on a promoter' : 'are waiting on promoters'}`,
		onDjs && `${pluralize(onDjs, 'booking')} ${onDjs === 1 ? 'is waiting on a DJ' : 'are waiting on DJs'}`,
		requests.length && `${pluralize(requests.length, 'request')} ${requests.length === 1 ? 'is' : 'are'} with staff`
	].filter(Boolean);

	if (!parts.length) return null;

	return `${parts.join(' and ')}. Nothing needed from you.`;
}

const bookingNight = (thread: BookingThreadView, did: string): NightRow => {
	const isConfirmed = thread.status === 'confirmed';
	const act = thread.project?.name;

	return {
		key: thread.skey,
		date: dayOf(thread.showDate ?? ''),
		title: thread.title ?? 'Booking',
		detail: [
			act ? `Booking ${roleIn(thread, did) === 'manager' ? 'for' : 'with'} ${act}` : 'Booking',
			!isConfirmed && 'not confirmed'
		]
			.filter(Boolean)
			.join(' · '),
		href: `/dashboard/bookings/${thread.skey}`,
		tentative: !isConfirmed
	};
};

const plannedNight = (show: PlannerShowView): NightRow => ({
	key: show.rkey,
	date: dayOf(show.details.date ?? ''),
	title: show.details.title,
	detail: ['Planned', show.details.venue, pluralize(show.lineup.length, 'DJ')].filter(Boolean).join(' · '),
	href: `/dashboard/planner/shows/${show.rkey}`,
	tentative: true
});

const closeness = (date: string, now: number): NightRow['when'] => {
	if (date === today(now)) return 'Tonight';

	return date === today(now + 86_400_000) ? 'Tomorrow' : undefined;
};

/**
 * The viewer's next nights across shows they announced, bookings and their planner, soonest first. Each source can
 * fail alone: `failed` says some are missing, and the rest still show. A booking that became a show counts once, as
 * the show.
 */
export async function nextNights(
	{
		app,
		log
	}: {
		app: {
			shows: Pick<ShowService, 'forProject'>;
			planner: Pick<PlannerService, 'saves' | 'shows'>;
			projects: Pick<ProjectService, 'byUris'>;
		};
		log: Log;
	},
	viewer: Viewer,
	threads: BookingThreadView[],
	projects: ProjectView[] | null,
	sections: { promoter: boolean } | null,
	now = Date.now()
): Promise<{ nights: NightRow[]; more: number; failed: boolean }> {
	const start = today(now);

	const shows = await unlessOffline(log, () =>
		Promise.all((projects ?? []).slice(0, PROJECTS_READ).map((project) => app.shows.forProject(project)))
	);

	const planner = !sections || sections.promoter ? await loadPlanner({ app, log }, viewer) : null;

	const announced = (shows ?? []).flat().map(({ uri, rkey, record, project }): NightRow => ({
		key: uri,
		date: dayOf(record.startsAt),
		title: record.name,
		detail: [
			`You play ${formatDate(record.startsAt, { timeStyle: 'short', timeZone: 'UTC', hourCycle: 'h23' })} as ${project.profile.name}`,
			record.venue
		]
			.filter(Boolean)
			.join(' · '),
		href: `/dashboard/projects/${project.slug}/shows/${rkey}`,
		tentative: false
	}));

	const booked = threads.filter(
		(thread) => thread.showDate && ['open', 'negotiating', 'confirmed'].includes(thread.status)
	);

	const planned = (planner?.shows ?? []).filter((show) => show.details.date);

	const once = new Map<string, NightRow>();

	for (const night of [
		...announced,
		...booked.map((thread) => bookingNight(thread, viewer.did)),
		...planned.map(plannedNight)
	]) {
		const same = `${night.date} ${night.title.toLowerCase()}`;

		if (night.date >= start && !once.has(same)) once.set(same, { ...night, when: closeness(night.date, now) });
	}

	const nights = [...once.values()].sort((a, b) => a.date.localeCompare(b.date));

	return {
		nights: nights.slice(0, NIGHTS_SHOWN),
		more: Math.max(nights.length - NIGHTS_SHOWN, 0),
		failed: shows === null || projects === null || (!!planner && 'problem' in planner)
	};
}
