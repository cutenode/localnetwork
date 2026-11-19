import { error } from '@sveltejs/kit';
import type { EventSummary } from '#lib/events.ts';
import { eventPagePath } from '#lib/space-paths.ts';
import { requireFlag } from './http';
import type { PartyView } from './services/parties';

/** A party as its pages find it: the party, its people, what the viewer can do in it, and whether they're inside it. */
export type FoundParty = Awaited<ReturnType<typeof findParty>>;

/**
 * The party with this key, its people as the viewer may see them, and what the viewer can do on it. Its people and
 * staff are `inside` it. A draft is only there for them; to anyone else it's not found, the same as a party that
 * doesn't exist.
 */
export async function findParty(locals: App.Locals, key: string) {
	requireFlag(locals, 'spacePages');
	const { app, viewer } = locals;
	const party = await app.parties.bySkey(key);

	if (!party) error(404, 'Not found');
	const people = await app.sharedSpaces.people(party.space, viewer);
	const person = people.find((someone) => someone.did === viewer?.did);
	const inside = !!person || !!viewer?.isStaff;

	// A draft is only its people's and staff's to see.
	if (!party.publishedAt && !inside) error(404, 'Not found');

	return { party, people, can: person?.capabilities ?? [], inside };
}

/**
 * The party's events whose night isn't over yet, soonest first, with drafts that have no date yet last. Visitors get
 * the published public ones; its people and staff (`inside`) get its drafts and private ones too, and the ones never
 * confirmed, which stay here for them.
 */
export async function upcomingEvents(
	app: Pick<App.Locals['app'], 'events'>,
	party: Pick<PartyView, 'space' | 'skey'>,
	inside: boolean
): Promise<EventSummary[]> {
	const events = await app.events.ofParty(party.space);

	return events.flatMap(({ rkey, record, venue, over, neverConfirmed }) => {
		const isPrivate = record.audience !== 'public';
		const isShown = inside ? !over || neverConfirmed : !over && !!record.publishedAt && !isPrivate;

		if (!isShown) return [];

		return [
			{
				href: eventPagePath(party.skey, rkey, record.name),
				name: record.name,
				startsAt: record.startsAt,
				status: record.status,
				venue: venue?.name ?? null,
				draft: !record.publishedAt,
				private: isPrivate,
				neverConfirmed
			}
		];
	});
}
