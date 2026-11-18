import type { EventRecord, ShowView } from './types';
import type { VenueOption } from './venues';

/**
 * Someone on an event's lineup as its page shows them: their name, their page when they have one (a DJ's profile; acts
 * get theirs later), what the lineup editor puts in its field for them, and their set times on the venue's clock.
 */
export type LineupEntry = { name: string; href: string | null; value: string; start?: string; end?: string };

/** An event as its page draws it. The link token never reaches the page; its people get the whole link instead. */
export type EventPageView = Pick<EventRecord, 'name' | 'description' | 'startsAt' | 'endsAt' | 'tickets' | 'status'> & {
	audience: 'public' | 'private';
	venue: VenueOption | null;
	/** The IANA time zone its times are on: its venue's, or while it has none, its own. Null for a venue with no location. */
	timeZone: string | null;
	/** Whether the venue has confirmed it, the only standing a page shows besides waiting for the venue (ADR 0014). */
	confirmed: boolean;
	/** Its night ended while it waited for the venue, which only its party's people see. */
	neverConfirmed: boolean;
	lineup: LineupEntry[];
};

/** One of a party's events as its page lists it. Its people also see its drafts, private ones and ones never confirmed, marked. */
export type EventSummary = Pick<EventRecord, 'name' | 'startsAt' | 'status'> & {
	href: string;
	venue: string | null;
	draft: boolean;
	private: boolean;
	neverConfirmed: boolean;
};

/**
 * A party's event as a list of upcoming ones shows it, beside announced shows: its page, its party's and venue's, and
 * whether the venue has confirmed it. `distanceKm` is how far its venue is from the place the list is near.
 */
export type ListedEvent = Pick<EventRecord, 'name' | 'endsAt' | 'status' | 'tickets'> & {
	kind: 'event';
	uri: string;
	href: string;
	startsAt: string;
	party: { name: string; href: string };
	venue: { name: string; href: string; locality?: string };
	confirmed: boolean;
	distanceKm?: number;
};

/** A row of a list of upcoming nights: a show a project announced, or a party's event. */
export type Listing = ShowView | ListedEvent;
