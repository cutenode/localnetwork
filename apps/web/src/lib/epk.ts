import type { z } from 'zod';
import type { epkContentSchema, epkFieldsSchema, epkRecordSchema } from './epk-schema.ts';
import type { AccountView } from './types.ts';

/**
 * Press kits (EPKs) as records: a public `computer.localnetwork.epk` in the root account's repo, and a
 * space per press kit for the rest, which people see once one of the project's managers grants them
 * access. Managers change them through drafts. These are the shapes the pages and the server share,
 * and the access rules both sides read the same way.
 */

/** What happens when someone in an audience asks for access. */
export const ACCESS_RULES = ['auto', 'review', 'none'] as const;

export type AccessRule = (typeof ACCESS_RULES)[number];

export const AUDIENCES = ['bookedWith', 'following', 'promoters', 'anyone'] as const;

export type Audience = (typeof AUDIENCES)[number];

/** Who can ask for a press kit's private part, and whether they get it straight away. */
export type EpkAccess = Record<Audience, AccessRule>;

export const AUDIENCE_LABELS: Record<Audience, { label: string; hint: string }> = {
	bookedWith: { label: "People you've booked with", hint: 'Anyone with a booking thread for this project' },
	following: { label: 'People you follow', hint: 'Accounts you follow on Bluesky' },
	promoters: { label: 'Promoters', hint: 'Anyone who has sent a booking inquiry on Local Network' },
	anyone: { label: 'Anyone signed in', hint: 'Every other account' }
};

export const RULE_LABELS: Record<AccessRule, string> = {
	auto: 'Approved automatically',
	review: 'You approve',
	none: "Can't ask"
};

/** The ladder from fully open to closed, most open first. Anything else is a custom mix. */
export const ACCESS_PRESETS: { id: string; label: string; description: string; access: EpkAccess }[] = [
	{
		id: 'open',
		label: 'Open',
		description: 'Anyone signed in gets access as soon as they ask.',
		access: { bookedWith: 'auto', following: 'auto', promoters: 'auto', anyone: 'auto' }
	},
	{
		id: 'promoters',
		label: 'Promoters automatically',
		description: 'Promoters and people you know get in at once. You approve everyone else.',
		access: { bookedWith: 'auto', following: 'auto', promoters: 'auto', anyone: 'review' }
	},
	{
		id: 'known',
		label: 'People you know automatically',
		description: "People you've booked with or follow get in at once. You approve everyone else.",
		access: { bookedWith: 'auto', following: 'auto', promoters: 'review', anyone: 'review' }
	},
	{
		id: 'review',
		label: 'You approve everyone',
		description: 'Anyone signed in can ask, and you decide each time.',
		access: { bookedWith: 'review', following: 'review', promoters: 'review', anyone: 'review' }
	},
	{
		id: 'promoters-ask',
		label: 'Only promoters can ask',
		description: 'Promoters and people you know can ask, and you decide. Nobody else can.',
		access: { bookedWith: 'review', following: 'review', promoters: 'review', anyone: 'none' }
	},
	{
		id: 'known-ask',
		label: 'Only people you know can ask',
		description: "People you've booked with or follow can ask, and you decide. Nobody else can.",
		access: { bookedWith: 'review', following: 'review', promoters: 'none', anyone: 'none' }
	},
	{
		id: 'closed',
		label: 'Nobody can ask',
		description: 'Only the people you add yourself get access.',
		access: { bookedWith: 'none', following: 'none', promoters: 'none', anyone: 'none' }
	}
];

export const DEFAULT_ACCESS: EpkAccess = ACCESS_PRESETS.find((preset) => preset.id === 'review')!.access;

/** The preset these rules match, or null for a custom mix. */
export function presetFor(access: EpkAccess) {
	return (
		ACCESS_PRESETS.find((preset) => AUDIENCES.every((audience) => preset.access[audience] === access[audience])) ?? null
	);
}

const OPENNESS: Record<AccessRule, number> = { none: 0, review: 1, auto: 2 };

/** The rule for someone in these audiences: the most open of theirs, since everyone signed in is in `anyone`. */
export function ruleFor(access: EpkAccess, audiences: Iterable<Audience>): AccessRule {
	let best: AccessRule = access.anyone;

	for (const audience of audiences) if (OPENNESS[access[audience]] > OPENNESS[best]) best = access[audience];

	return best;
}

/** A rule as a record states it; one this app doesn't know grants nothing. */
export function readRule(value: string | undefined): AccessRule {
	return ACCESS_RULES.find((rule) => rule === value) ?? 'none';
}

export type EpkRecord = z.infer<typeof epkRecordSchema>;

export type EpkContent = z.infer<typeof epkContentSchema>;

export type EpkMix = NonNullable<EpkRecord['mixes']>[number];

export type EpkPhoto = NonNullable<EpkRecord['photos']>[number];

export type EpkQuote = NonNullable<EpkContent['quotes']>[number];

export const REQUEST_ROLES = ['promoter', 'venue', 'agent', 'press', 'artist', 'other'] as const;

export type RequestRole = (typeof REQUEST_ROLES)[number];

export const ROLE_LABELS: Record<RequestRole, string> = {
	promoter: 'Promoter',
	venue: 'Venue',
	agent: 'Agent',
	press: 'Press',
	artist: 'Artist',
	other: 'Something else'
};

export type EpkView = {
	uri: string;
	/** The repo holding it: the root's, or for a legacy press kit, the manager's who made it. */
	did: string;
	rkey: string;
	/** Changes on every publish, so it identifies the published version. */
	cid: string;
	space: string;
	/** Made before the root held press kits, and still in its maker's repo until their next visit to the dashboard. */
	legacy: boolean;
	record: EpkRecord;
};

/** A project's mixes and press kits are kept to the same size. */
export const MAX_EPKS_PER_PROJECT = 20;

/** What a manager is told when they try to change a press kit that hasn't moved to the root yet. */
export const LEGACY_KIT =
	'This press kit moves to the project the next time the manager who made it opens the dashboard';

/** Some fields of a press kit: the public record's at the top level, the private part's under `content`. */
export type EpkFields = z.infer<typeof epkFieldsSchema>;

type PublicValues = Omit<EpkFields, 'content'>;

type ContentValues = NonNullable<EpkFields['content']>;

/** What one field of a press kit holds. */
export type EpkFieldValue =
	| NonNullable<PublicValues[keyof PublicValues]>
	| NonNullable<ContentValues[keyof ContentValues]>;

export const EPK_PUBLIC_FIELDS = ['title', 'headline', 'bio', 'mixes', 'photos', 'access'] as const;

export const EPK_CONTENT_FIELDS = ['bio', 'mixes', 'photos', 'quotes', 'contact', 'techRider'] as const;

/** One field of a press kit, as a draft changes it: `title`, or `content.bio` for the private part's. */
export type EpkField = (typeof EPK_PUBLIC_FIELDS)[number] | `content.${(typeof EPK_CONTENT_FIELDS)[number]}`;

/**
 * Where a draft stands against the published press kit: a draft of one that isn't published yet, one
 * made from the version published now, one made before someone published another, or one whose press
 * kit was deleted since.
 */
export type EpkDraftStatus = 'new' | 'current' | 'behind' | 'deleted';

export type EpkDraftView = {
	uri: string;
	/** The press kit's key: the published one's, or the one a new press kit gets. */
	rkey: string;
	author: AccountView;
	/** The repo holding the draft, where its new photos are: the author's, or the root's on their behalf. */
	repo: string;
	base: { uri: string; cid: string } | null;
	changes: EpkFields;
	previous: EpkFields;
	/** The fields it changes. */
	fields: EpkField[];
	status: EpkDraftStatus;
	createdAt: string;
	updatedAt: string;
};

/** A project's press kits as its dashboard lists them, with the managers' drafts of each, then drafts of unpublished ones. */
export type EpkDashboard = { kits: { epk: EpkView; drafts: EpkDraftView[] }[]; drafts: EpkDraftView[] };

export const DRAFT_STATUS_LABELS: Record<EpkDraftStatus, string> = {
	new: 'New',
	current: 'Up to date',
	behind: 'Behind',
	deleted: 'Deleted'
};

/** A field someone else published since a draft changed it: its value now, when the draft changed it, and in the draft. */
export type EpkConflict = {
	field: EpkField;
	published: EpkFieldValue | undefined;
	previous: EpkFieldValue | undefined;
	draft: EpkFieldValue | undefined;
};

/** A conflict as the dashboard shows it: the field's name, and each value written out. */
export type EpkConflictView = { field: EpkField; label: string; published: string; previous: string; draft: string };

/** A press kit's public page. */
export const pressKitPath = (slug: string, rkey: string) => `/projects/${slug}/press-kits/${rkey}`;

/** What publishing a draft did: published it, or found fields someone else published since, and wrote nothing. */
export type EpkPublishResult = { kind: 'published'; epk: EpkView } | { kind: 'conflict'; conflicts: EpkConflict[] };

export type EpkRequestStatus = 'pending' | 'approved' | 'declined' | 'withdrawn';

export type EpkRequestView = {
	uri: string;
	requester: AccountView;
	role: RequestRole | null;
	message: string | null;
	status: EpkRequestStatus;
	/** What the press kit's rules say for this person: auto requests of a legacy press kit wait for it to move. */
	rule: AccessRule;
	createdAt: string;
	decidedAt: string | null;
};

export type EpkGrantView = {
	subject: AccountView;
	via: 'auto' | 'approved' | 'invited';
	createdAt: string;
};

/** A receipt of someone with access viewing a press kit's private part, for its project's managers. */
export type EpkViewReceipt = {
	/** The audit entry recording the view. */
	id: string;
	viewer: AccountView;
	epk: { uri: string; rkey: string; title: string };
	viewedAt: string;
};

/** Where the viewer stands with a press kit's private part. */
export type EpkAccessState =
	| { kind: 'manager' }
	| { kind: 'granted'; since: string }
	| { kind: 'signedOut'; rule: AccessRule }
	| { kind: 'pending'; since: string; approved: boolean }
	| { kind: 'declined' }
	| { kind: 'canAsk'; rule: AccessRule }
	| { kind: 'closed' };

/** Where something in a press kit shows: to everyone, to people with access, or nowhere. */
export type Visibility = 'public' | 'private' | 'hidden';

export type EpkFormPhoto = {
	cid: string;
	alt: string;
	credit: string;
	visibility: Visibility;
	width?: number;
	height?: number;
};

/** What the press kit form shows, from saved records or from a submission that failed. */
export type EpkFormValues = {
	/** The version of the public record the form was opened on, which a save swaps on; empty for a new press kit. */
	cid: string;
	title: string;
	headline: string;
	bio: string;
	longBio: string;
	preset: string;
	access: EpkAccess;
	/** The project's mixes, each shown to everyone, on request or not at all. */
	mixes: { title: string; url: string; visibility: Visibility }[];
	/** Mixes only in this press kit, like unreleased promos. */
	extraMixes: { title: string; url: string; visibility: Visibility }[];
	photos: EpkFormPhoto[];
	quotes: EpkQuote[];
	email: string;
	phone: string;
	contactNote: string;
	techRider: string;
};

/** The long edge press asks for, and the smallest that still works on the web. */
export const PRESS_LONG_EDGE = 3000;

export const WEB_LONG_EDGE = 1440;

export function photoSize(width?: number, height?: number): 'press' | 'web' | 'small' | null {
	if (!width || !height) return null;
	const edge = Math.max(width, height);

	return edge >= PRESS_LONG_EDGE ? 'press' : edge >= WEB_LONG_EDGE ? 'web' : 'small';
}

export function orientation(width: number, height: number): 'landscape' | 'portrait' | 'square' {
	const ratio = width / height;

	return ratio > 1.05 ? 'landscape' : ratio < 0.95 ? 'portrait' : 'square';
}

/**
 * What press and promoters ask of a set of photos: 3 to 5 of them, every orientation so nothing
 * gets cropped, 3,000 px on the long edge, and a credit for the photographer.
 */
export function pressChecks(photos: { width?: number; height?: number; credit?: string }[]) {
	const sized = photos.filter((photo) => photo.width && photo.height);
	const orientations = new Set(sized.map((photo) => orientation(photo.width!, photo.height!)));

	return [
		{ label: '3 or more photos', done: photos.length >= 3 },
		{ label: 'Landscape, portrait and square', done: orientations.size === 3 },
		{
			label: '3,000 px on the long edge',
			done: photos.length > 0 && photos.every((photo) => photoSize(photo.width, photo.height) === 'press')
		},
		{ label: 'Every photo credited', done: photos.length > 0 && photos.every((photo) => photo.credit) }
	];
}

/** An empty press kit form: no mixes on the project yet, and the default rules. */
export const epkFormDefaults: EpkFormValues = {
	cid: '',
	title: '',
	headline: '',
	bio: '',
	longBio: '',
	preset: 'review',
	access: DEFAULT_ACCESS,
	mixes: [],
	extraMixes: [],
	photos: [],
	quotes: [],
	email: '',
	phone: '',
	contactNote: '',
	techRider: ''
};
