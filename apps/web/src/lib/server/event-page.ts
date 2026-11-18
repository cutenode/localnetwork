import { error, fail, redirect } from '@sveltejs/kit';
import type { Capability } from '#lib/capabilities.ts';
import type { PageStatus } from '#lib/components/edit/types.ts';
import type { EventPageView, LineupEntry } from '#lib/events.ts';
import { accountLabel, profileHref } from '#lib/format.ts';
import { COLLECTION } from '#lib/nsid.ts';
import { venueClock } from '#lib/shows.ts';
import { eventPagePath, spacePagePath } from '#lib/space-paths.ts';
import type { EventCredit, Viewer } from '#lib/types.ts';
import { spaceRecordUri } from './atproto/spaces';
import { openEditor, pageContext, savePage } from './edit-mode';
import { eventValues, parseEventForm, zoneChoice } from './forms/event';
import { act, cacheBriefly, requireViewer } from './http';
import { pageMeta } from './meta';
import type { FoundParty } from './party-page';
import { parsePerformerUri } from './performer-uri';
import { type EventView, venueZone, ZoneChangeError } from './services/events';
import { venueOption } from './services/venues';

type Locals = App.Locals;

/** The query a private event's link carries its token in. */
export const LINK_TOKEN = 't';

/**
 * Every IANA time zone a draft without a venue can keep its times on, UTC first, for its editor's list. The server
 * sends it, so the list the page hydrates with is the one it was drawn with.
 */
export const TIME_ZONES = ['UTC', ...Intl.supportedValuesOf('timeZone').filter((zone) => zone !== 'UTC')];

/**
 * The party's event that `rest` (what follows the party's key) starts with: any of them for its people and staff, and
 * for anyone else only as its link opens it (ADR 0015), or else not found. Null when `rest` names none of the party's
 * events, so it's the party's own name.
 */
export async function findEvent(
	locals: Locals,
	{ party, inside }: FoundParty,
	rest: string,
	token: string | null
): Promise<EventView | null> {
	const { events, core } = locals.app;
	const uri = spaceRecordUri(party.space, core.rootDid, COLLECTION.event, rest.split('/')[0]);
	const event = await events.get(uri);

	if (!event || inside) return event;

	return (await events.open(uri, token)) ?? error(404, 'Not found');
}

/** On an event, editing its page is managing the party's events, whatever the party's own page.edit says. */
const eventCan = (can: Capability[]): Capability[] => [
	...can.filter((capability) => capability !== 'page.edit'),
	...(can.includes('events.manage') ? (['page.edit'] as const) : [])
];

/** Someone on the lineup by what names them: a DJ by their handle and profile, an act by its name and page link. */
async function lineupEntry(app: Locals['app'], credit: EventCredit): Promise<LineupEntry> {
	const ref = parsePerformerUri(app.core.rootDid, credit.performer);

	const times = {
		start: credit.startsAt && venueClock(credit.startsAt),
		end: credit.endsAt && venueClock(credit.endsAt)
	};

	if (ref?.kind === 'dj') {
		const account = await app.accounts.view(ref.did);

		return { name: accountLabel(account), href: profileHref(account), value: account.handle ?? account.did, ...times };
	}

	const act = ref?.kind === 'act' ? await app.performerRegistry.byKey(ref.skey) : null;

	if (act?.kind === 'act') {
		const { name } = act.act.basics;

		return { name, href: null, value: spacePagePath('act', act.skey, name), ...times };
	}

	// Whoever it named has left the site, and the event's service won't save them again, so their row is empty.
	return { name: 'Someone no longer here', href: null, value: '', ...times };
}

/** The event as its page draws it, without its link token. */
async function eventView(app: Locals['app'], { record, venue, standing }: EventView): Promise<EventPageView> {
	const { name, description, startsAt, endsAt, tickets, status } = record;

	return {
		name,
		description,
		startsAt,
		endsAt,
		tickets,
		status,
		audience: record.audience === 'public' ? 'public' : 'private',
		venue: venue && venueOption(venue),
		timeZone: (venue ? venueZone(venue) : record.timeZone) ?? null,
		confirmed: standing.confirmed,
		lineup: await Promise.all((record.lineup ?? []).map((credit) => lineupEntry(app, credit)))
	};
}

/**
 * `/party/<key>/<event>/<name>`: the event's page, built in place like its party's. The name part is only there to
 * read, so any other is sent on to the current one, keeping the query (and so a private event's link token). The
 * party's people get the edit bar, those with events.manage its Edit and, for a private event, its whole link to share.
 */
export async function eventPage(
	event: { locals: Locals; url: URL; setHeaders(headers: Record<string, string>): void },
	{ party, can }: FoundParty,
	shown: EventView
) {
	const { locals, url } = event;
	const { record } = shown;
	const path = eventPagePath(party.skey, shown.rkey, record.name);

	if (url.pathname !== path) redirect(308, `${path}${url.search}`);
	const isPrivate = record.audience !== 'public';
	const context = pageContext(locals, 'event', shown.uri, eventCan(can));
	const status: PageStatus = !record.publishedAt ? 'draft' : isPrivate ? 'private' : 'listed';

	// A private event's page answers to its token, so nothing keeps a copy that would outlast a reset.
	if (isPrivate) event.setHeaders({ 'cache-control': 'no-store', vary: 'Cookie' });
	else cacheBriefly(event, 60, context.can ?? []);

	const meta = pageMeta(locals.app.config.publicUrl, {
		title: record.name,
		description: record.description ?? `${record.name}, a night by ${party.basics.name}.`,
		path,
		noindex: isPrivate || !record.publishedAt
	});

	const editing = await openEditor(locals, url, context, record.name);

	return {
		...context,
		editing,
		timeZones: editing ? TIME_ZONES : [],
		version: shown.cid,
		status,
		event: await eventView(locals.app, shown),
		party: { name: party.basics.name, href: spacePagePath('party', party.skey, party.basics.name) },
		// Only a private event has a token, and only those who manage it are handed it.
		link:
			context.can?.includes('events.manage') && record.linkToken
				? `${meta.url}?${LINK_TOKEN}=${record.linkToken}`
				: null,
		meta
	};
}

type ActionEvent = { locals: Locals; request: Request; url: URL };

/** The event page's one save, over the version its form opened on. A draft is published by the save that asks. */
export async function saveEvent(event: ActionEvent, shown: EventView) {
	const { locals, request, url } = event;
	const viewer = requireViewer(locals, url);
	const form = await request.formData();
	let asked: ZoneChangeError | undefined;

	const result = await savePage(
		event,
		form,
		async (version) => {
			try {
				await locals.app.events.update(viewer, shown.uri, {
					...(await parseEventForm(locals.app, form)),
					swap: version
				});
			} catch (cause) {
				if (cause instanceof ZoneChangeError) asked = cause;
				throw cause;
			}
		},
		async () => {
			// SAFETY: a stale save read the event's record before refusing, and indexed every save before it.
			const latest = (await locals.app.events.get(shown.uri))!;

			return { version: latest.cid, values: eventValues(latest.record) };
		}
	);

	if (!asked) return result;
	locals.log.info('an event save asked how its times move to another clock', { uri: shown.uri, to: asked.to });

	// The times would move onto another clock, so the editor picks how: each choice saves again with zoneChange.
	const choice = zoneChoice(asked, !!form.get('venue'));

	// Its field is the choice, so the editor puts focus on the first way to answer it.
	return fail(400, { ...result.data, error: 'Pick which times to keep', field: { name: choice.name, row: 0 }, choice });
}

/** One of the event's own actions outside edit mode (cancelling it, resetting its link), for its people. */
export async function changeEvent(event: ActionEvent, change: (viewer: Viewer, form: FormData) => Promise<void>) {
	const viewer = requireViewer(event.locals, event.url);
	const form = await event.request.formData();

	return act(() => change(viewer, form));
}
