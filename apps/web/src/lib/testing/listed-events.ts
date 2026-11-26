import type { ListedEvent } from '#lib/events.ts';
import { EVENT_STATUS } from '#lib/shows.ts';

/** One of Night Shift's events at Tresor as lists show it, waiting for the venue unless `overrides` say otherwise. */
export const listedEvent = (rkey: string, overrides: Partial<ListedEvent> = {}): ListedEvent => ({
	kind: 'event',
	uri: `at:space/did:plc:root/computer.localnetwork.space.party/3lzparty/did:plc:root/computer.localnetwork.event/${rkey}`,
	href: `/party/3lzparty/${rkey}/winter-opening`,
	name: 'Winter Opening',
	startsAt: '2026-12-05T23:00:00+01:00',
	endsAt: '2026-12-06T06:00:00+01:00',
	status: EVENT_STATUS.scheduled,
	party: { name: 'Night Shift', href: '/party/3lzparty/night-shift' },
	venue: { name: 'Tresor', href: '/venue/3tresor/tresor', locality: 'Berlin' },
	confirmed: false,
	...overrides
});
