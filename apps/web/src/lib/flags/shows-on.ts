/**
 * The kinds of page a flag can show on (its `showsOn` in flags/catalog), with what the staff console calls each:
 * `global` for every page, or one of the glossary's page types. `act` is `/projects/<slug>` until acts move.
 */
export const SHOWS_ON = {
	global: 'Every page',
	profile: 'Profile',
	act: 'Act',
	party: 'Party',
	venue: 'Venue',
	event: 'Event',
	set: 'Set',
	mix: 'Mix',
	pressKit: 'Press kit'
} as const;

export type ShowsOn = keyof typeof SHOWS_ON;

// SAFETY: the keys of a const object literal.
export const SHOWS_ON_KINDS = Object.keys(SHOWS_ON) as [ShowsOn, ...ShowsOn[]];
