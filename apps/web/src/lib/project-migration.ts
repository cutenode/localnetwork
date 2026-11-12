import { feeLevels } from './format';
import type { ActForm, ActSubject, PerformerRecord, ProjectProfile } from './types';

/** How a solo project's DJ was found: their own `self` claim, having created it, staff naming them, or not at all. */
export type DjRule = 'self' | 'createdBy' | 'override' | 'unresolved';

/** Someone the performer space would hold, and the role they'd get. */
export type PlannedPerson = {
	did: string;
	handle: string | null;
	role: 'member' | 'manager';
	relationship: string | null;
};

/** A project profile field the performer record has no place for, and what happens to it. */
export type UnmappedField = { field: keyof ProjectProfile; reason: string };

/** What stops a project from moving until staff sort it out. */
export type MigrationBlocker =
	| { kind: 'legacyPressKit'; uri: string; did: string }
	| { kind: 'unresolvedDj' }
	| { kind: 'slugTaken'; slug: string; space: string }
	| { kind: 'djSpaceTaken'; did: string; space: string }
	| { kind: 'noManagers' };

/** A request that would be declined with a note, since the project it names is going away. */
export type PendingRequest = { uri: string; kind: 'create' | 'claim' | 'update'; requester: string; createdAt: string };

/** What moving one project to a performer would do. */
export type ProjectPlan = {
	uri: string;
	slug: string;
	name: string;
	target: 'act' | 'dj';
	/** A DJ's DID and how it was found; null for an act. */
	dj: { did: string | null; handle: string | null; rule: DjRule } | null;
	/** The performer space, keyed by the project's own key. */
	space: string;
	/** What a press kit's `project` becomes; null for a DJ nobody resolved. */
	performerUri: string | null;
	/** The record the root would write into the space; null for a DJ nobody resolved. */
	record: PerformerRecord | null;
	people: PlannedPerson[];
	listed: boolean;
	/** Whether there are booking details to copy into the performer's booking details space. */
	hasDetails: boolean;
	drafts: { uri: string; author: string; epkRkey: string; updatedAt: string }[];
	/** The root's press kits whose `project` would change to `performerUri`. */
	pressKits: { uri: string; title: string }[];
	bookingThreads: { space: string; title: string | null; status: string }[];
	/** Records in other people's repos that name the project, left for a lazy rewrite. */
	lazy: { follows: number; upcomingShows: number };
	decline: PendingRequest[];
	/** `from/**` redirects to `to/**`; null when there's nowhere to send it yet. */
	redirect: { from: string; to: string | null };
	unmapped: UnmappedField[];
	blockers: MigrationBlocker[];
};

export type MigrationPlan = {
	projects: ProjectPlan[];
	/** Create requests name no project, so they're listed once for the whole site. */
	pendingCreates: PendingRequest[];
	totals: { acts: number; djs: number; blocked: number };
};

/** Where each project profile field lands on an act. Typed over every key, so a new profile field has to be placed. */
export const FIELD_HOMES = {
	name: 'basics.name',
	kind: 'form',
	tagline: 'performer.tagline',
	bio: 'basics.bio',
	pronouns: 'basics.pronouns',
	genres: 'performer.genres',
	location: 'basics.location',
	travel: 'performer.travel',
	formats: 'performer.formats',
	setTypes: 'performer.setTypes',
	setLength: 'performer.setLength',
	activeSince: 'performer.activeSince',
	capacity: 'performer.capacity',
	fees: 'performer.fees',
	fee: 'performer.fees, when fees is empty',
	links: 'basics.links',
	mixes: 'performer.mixes',
	highlights: 'performer.highlights',
	lookingFor: 'performer.lookingFor',
	avatar: 'basics.avatar'
} as const satisfies Record<keyof ProjectProfile, string>;

/** The act forms a project's kind names the same way. A solo project becomes a DJ, not an alias. */
const SHARED_FORMS = ['duo', 'group', 'collective'] as const satisfies readonly ActForm[];

/** An act's subject from a project's profile, and the fields it can't carry. */
export function actSubject(slug: string, profile: ProjectProfile) {
	const { kind, fee, fees: _fees, name, bio, pronouns, location, links, avatar, ...performer } = profile;
	const form = SHARED_FORMS.find((shared) => shared === kind);
	const fees = feeLevels(profile);
	const unmapped: UnmappedField[] = [];

	if (!form) unmapped.push({ field: 'kind', reason: `An act has no "${kind}" form, so it's planned as a group` });

	if (fee && profile.fees) unmapped.push({ field: 'fee', reason: 'Dropped: fees already lists the fee levels' });

	return {
		subject: {
			slug,
			form: form ?? 'group',
			basics: { name, avatar, bio, pronouns, location, links },
			performer: fees.length ? { ...performer, fees } : performer
		} satisfies ActSubject,
		unmapped
	};
}

/**
 * A DJ's page is their own profile, which only they can write, so none of the project's fields carry over. In
 * FIELD_HOMES order, whatever order the record had.
 */
export const djUnmapped = (profile: ProjectProfile): UnmappedField[] =>
	Object.keys(FIELD_HOMES)
		.filter(isProfileField)
		.filter((field) => profile[field] !== undefined)
		.map((field) => ({ field, reason: "Not copied: the DJ's own profile is their page" }));

const isProfileField = (key: string): key is keyof ProjectProfile => Object.hasOwn(FIELD_HOMES, key);
