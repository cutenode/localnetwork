/** What a shared space is for. Each kind has a space type of its own, and the capabilities and roles below. */
export const SPACE_KINDS = ['performer', 'party', 'venue'] as const;

export type SpaceKind = (typeof SPACE_KINDS)[number];

type CapabilitySpec = { kinds: readonly SpaceKind[]; write: boolean };

/**
 * Everything someone can do for a performer, party or venue, and which kinds offer it. A capability that `write`s has
 * its holder write records into the space, so it makes them a writing member; the site does the rest for them as the
 * root. The people defs lexicon lists the same ids, in this order, as #capability's knownValues.
 */
export const CAPABILITIES = {
	'page.edit': { kinds: ['performer', 'party', 'venue'], write: true },
	'people.manage': { kinds: ['performer', 'party', 'venue'], write: false },
	'listing.request': { kinds: ['performer', 'party'], write: false },
	'presskits.manage': { kinds: ['performer'], write: true },
	'bookings.answer': { kinds: ['performer'], write: false },
	'shows.manage': { kinds: ['performer'], write: true },
	'events.manage': { kinds: ['party'], write: true },
	'bookings.send': { kinds: ['party'], write: false },
	'guests.manage': { kinds: ['party'], write: false },
	'guests.checkin': { kinds: ['party'], write: false },
	'venue.approve': { kinds: ['venue'], write: false },
	'venue.block': { kinds: ['venue'], write: false }
} as const satisfies Record<string, CapabilitySpec>;

export type Capability = keyof typeof CAPABILITIES;

// SAFETY: Object.keys of the object literal above, whose keys are exactly Capability.
const ALL = Object.keys(CAPABILITIES) as Capability[];

const offeredBy = (kind: SpaceKind, capability: Capability) =>
	CAPABILITIES[capability].kinds.some((offering) => offering === kind);

/**
 * The capabilities in `values` that `kind` offers, once each, in catalog order, so the same set always reads the
 * same. Anything else, like a capability a newer version of the app added, is dropped.
 */
export function capabilitiesIn(kind: SpaceKind, values: readonly string[]): Capability[] {
	return ALL.filter((capability) => values.includes(capability) && offeredBy(kind, capability));
}

const every = (kind: SpaceKind) => ALL.filter((capability) => offeredBy(kind, capability));

type RolePreset = {
	capabilities: readonly Capability[];
	/** Whether people in the role appear in the space's public listing, unless they hide. */
	listed: boolean;
};

/**
 * Each kind's roles, the named starting sets of capabilities a person gets. A person's capabilities can change after
 * that; the record keeps both. The people defs lexicon lists every role name as #role's knownValues.
 */
export const ROLES = {
	performer: {
		member: { capabilities: every('performer'), listed: true },
		manager: {
			capabilities: ['page.edit', 'listing.request', 'presskits.manage', 'bookings.answer', 'shows.manage'],
			listed: false
		}
	},
	party: {
		host: { capabilities: every('party'), listed: true },
		cohost: { capabilities: every('party').filter((capability) => capability !== 'people.manage'), listed: true },
		door: { capabilities: ['guests.checkin'], listed: false }
	},
	venue: {
		owner: { capabilities: every('venue'), listed: false },
		booker: { capabilities: ['venue.approve'], listed: false }
	}
} as const satisfies { [K in SpaceKind]: Record<string, RolePreset> };

export type Role<K extends SpaceKind = SpaceKind> = K extends SpaceKind ? keyof (typeof ROLES)[K] & string : never;

export function isRole<K extends SpaceKind>(kind: K, value: string): value is Role<K> {
	return Object.hasOwn(ROLES[kind], value);
}

/** The capabilities `role` starts with, in catalog order. */
export function capabilitiesFor<K extends SpaceKind>(kind: K, role: Role<K>): Capability[] {
	const roles: Record<string, RolePreset> = ROLES[kind];

	return capabilitiesIn(kind, roles[role].capabilities);
}

/** Whether someone in `role` (a role of `kind`, or any other value) appears in the public listing. */
export function isListedRole(kind: SpaceKind, role: string): boolean {
	const roles: Record<string, RolePreset> = ROLES[kind];

	return Object.hasOwn(roles, role) && roles[role].listed;
}

/** Whether someone with these capabilities writes to the space, rather than only reading it. */
export function isWriter(capabilities: readonly Capability[]): boolean {
	return capabilities.some((capability) => CAPABILITIES[capability].write);
}

/** The role someone gets for the space they start. */
export const CREATOR_ROLE = { performer: 'member', party: 'host', venue: 'owner' } as const satisfies {
	[K in SpaceKind]: Role<K>;
};

/** What an invite link never gives: anyone can pass a link on, so it can't let them manage the space's people. */
export const LINK_FORBIDDEN: readonly Capability[] = ['people.manage'];
