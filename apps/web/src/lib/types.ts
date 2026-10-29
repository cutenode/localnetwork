/** View models shared between server and browser code. */

export type BlobRefJson = { $type: 'blob'; ref: { $link: string }; mimeType: string; size: number };

export type Location = {
	city: string;
	region?: string;
	country: string;
	latE6?: number;
	lngE6?: number;
};

export type ProjectLink = { kind: string; url: string; label?: string };

/** A `com.atproto.repo.strongRef`: a record and the version of it that was meant. */
export type StrongRef = { uri: string; cid: string };

export type ProjectMix = {
	title: string;
	url: string;
	durationMinutes?: number;
	year?: number;
	/** A locallibrary mix report (see `#lib/mix/report.ts`). */
	tracklist?: BlobRefJson;
	/** The manager's `computer.localnetwork.dj.set` this mix records. */
	set?: StrongRef;
};

/** A DJ's set as the directory serves it (`computer.localnetwork.directory.defs#setView`). */
export type DjSetView = StrongRef & {
	title?: string;
	/** YYYY-MM-DD */
	date: string;
	kind?: string;
	venue?: string;
	location?: { country: string; name?: string; locality?: string; region?: string };
	trackCount: number;
};

/** A city with sets, and how many (`computer.localnetwork.directory.defs#place`). */
export type DirectoryPlace = { city: string; country: string; count: number };

/** An artist a track credits (`#artistCredit`). `mbid` is a bare lowercase UUID. */
export type ArtistCredit = { name: string; mbid?: string };

/** An artist whose tracks the directory's DJs played, with their supports (`#supportedArtist`). */
export type SupportedArtist = ArtistCredit & { supports: number; djCount: number };

/** A track the directory's DJs played, and how many of them did (`#supportedTrack`). */
export type SupportedTrack = {
	id: string;
	title: string;
	artist?: string;
	artists: ArtistCredit[];
	recordingMbId?: string;
	isrc?: string;
	supports: number;
};

/** One time a DJ (the set's author) played a track (`#play`). */
export type TrackPlay = { did: string; set: DjSetView; startSeconds?: number };

/** A track and every time it was played (`#trackPlays`). */
export type TrackPlays = { track: SupportedTrack; plays: TrackPlay[] };

/** A DJ and the active projects they manage (`#djView`). */
export type DirectoryDj = { did: string; projects: { slug: string; name: string }[] };

/** What an artist's page shows: their supports, their tracks' plays, and the DJs by DID. No `artist` when no set credits them. */
export type ArtistSupports = {
	artist?: SupportedArtist;
	tracks: TrackPlays[];
	djs: Record<string, AccountView & DirectoryDj>;
};

/** The charts: the most supported tracks and artists, and the cities with sets. */
export type Charts = { tracks: SupportedTrack[]; artists: SupportedArtist[]; places: DirectoryPlace[] };

/** One fee level (`#fee`): its tier counts for budget matching; amounts are whole currency units. */
export type ProjectFee = { tier: string; label?: string; currency?: string; minAmount?: number; maxAmount?: number };

export type ProjectProfile = {
	name: string;
	kind: string;
	tagline?: string;
	bio?: string;
	pronouns?: string;
	genres: string[];
	location: Location;
	travel?: { radiusKm?: number; international?: boolean; notes?: string };
	formats?: string[];
	setTypes?: string[];
	setLength?: { minMinutes?: number; maxMinutes?: number };
	activeSince?: number;
	capacity?: { min?: number; max?: number };
	fees?: ProjectFee[];
	/** Deprecated: the one fee level of records written before `fees`. Read both through `feeLevels`. */
	fee?: ProjectFee;
	links?: ProjectLink[];
	mixes?: ProjectMix[];
	highlights?: string[];
	lookingFor?: string;
	avatar?: BlobRefJson;
};

export type ProjectDetails = {
	bookingEmail?: string;
	bookingPhone?: string;
	rateNotes?: string;
	techRider?: string;
	hospitality?: string;
};

/** A fee level's row in the project form, as the text its inputs hold. */
export type FeeFormRow = Record<'label' | 'tier' | 'currency' | 'minAmount' | 'maxAmount', string>;

/**
 * The project form's fields by input name, for filling the form back in: a saved profile from
 * `profileToValues`, or the submitted text from `formValues`, so number fields can be either.
 */
export type ProjectFormValues = ProjectDetails & {
	name?: string;
	kind?: string;
	tagline?: string;
	pronouns?: string;
	genres?: string;
	bio?: string;
	city?: string;
	region?: string;
	country?: string;
	latE6?: number | string;
	lngE6?: number | string;
	travelRadiusKm?: number | string;
	international?: boolean;
	travelNotes?: string;
	formats?: string[];
	setTypes?: string[];
	setMin?: number | string;
	setMax?: number | string;
	activeSince?: number | string;
	capacityMin?: number | string;
	capacityMax?: number | string;
	fees?: FeeFormRow[];
	links?: ProjectLink[];
	mixes?: ProjectMix[];
	highlights?: string;
	lookingFor?: string;
	message?: string;
	/** Fields only some forms have, like the admin form's `managers`, echo back as text too. */
	[field: string]: string | number | boolean | string[] | ProjectLink[] | ProjectMix[] | FeeFormRow[] | undefined;
};

export type ProjectStatus = 'active' | 'hidden' | 'archived';

export type ProjectRecord = {
	slug: string;
	profile: ProjectProfile;
	managers: string[];
	status: ProjectStatus;
	revision: number;
	createdBy: string;
	lastRequest?: string;
	createdAt: string;
	updatedAt: string;
};

export type ProjectView = {
	uri: string;
	rkey: string;
	cid: string;
	slug: string;
	profile: ProjectProfile;
	managers: string[];
	status: ProjectStatus;
	revision: number;
	createdBy: string;
	createdAt: string;
	updatedAt: string;
	avatarUrl: string | null;
	distanceKm?: number | null;
};

export type AccountView = { did: string; handle: string | null };

/**
 * A show (`computer.localnetwork.show`): a copy of the calendar event a manager announced, tied to the project. Its
 * times carry the venue's UTC offset, so their first 16 characters read as the venue's clock.
 */
export type ShowRecord = {
	project: string;
	event: StrongRef;
	name: string;
	startsAt: string;
	endsAt?: string;
	venue?: string;
	location: Location;
	status?: string;
	tickets?: string;
	createdAt: string;
};

/** A show as pages list it, with its project. `did` and `rkey` are the announcing manager's repo and its key. */
export type ShowView = {
	uri: string;
	cid: string;
	did: string;
	rkey: string;
	record: ShowRecord;
	project: ProjectView;
	distanceKm?: number;
};

/** A project's follower count, and whether the viewer is one of them. */
export type FollowState = { followers: number; following: boolean };

export type RequestKind = 'create' | 'claim' | 'update';

export type RequestStatus = 'pending' | 'approved' | 'rejected' | 'changes_requested' | 'withdrawn';

export type ClaimEvidence = { relationship: string; evidence: string; links?: string[] };

export type RequestRecord = {
	kind: RequestKind;
	project?: string;
	baseRevision?: number;
	profile?: ProjectProfile;
	details?: ProjectDetails;
	claim?: ClaimEvidence;
	message?: string;
	onBehalfOf?: string;
	withdrawnAt?: string;
	createdAt: string;
	updatedAt?: string;
};

export type RequestView = {
	uri: string;
	cid: string;
	rkey: string;
	kind: RequestKind;
	status: RequestStatus;
	requester: AccountView;
	repoDid: string;
	projectUri: string | null;
	project: { slug: string; name: string } | null;
	record: RequestRecord;
	createdAt: string;
	updatedAt: string;
	decision: { by: AccountView | null; at: string | null; reason: string | null } | null;
	resultProjectSlug: string | null;
};

export type StaffRole = 'admin' | 'moderator';

/** A Bluesky verification (`app.bsky.graph.verification`) the root account issued: who, under what handle and name. */
export type IssuedVerification = {
	uri: string;
	cid: string;
	rkey: string;
	subject: string;
	handle: string;
	displayName: string;
	createdAt: string;
};

/** An account's handle, when it points back to the account, and its Bluesky display name, as they are now. */
export type LiveIdentity = { handle: string | null; displayName: string | null };

/** Where an account stands with the root's verifications. `outdated` ones no longer match, so apps don't show them. */
export type VerificationStatus = 'verified' | 'outdated' | 'unverified';

export type Viewer = {
	did: string;
	handle: string | null;
	roles: StaffRole[];
	isAdmin: boolean;
	isStaff: boolean;
	spacesCapable: boolean;
};

export type FieldChange = { field: string; before?: string; after?: string };

export type ShowDetails = {
	title: string;
	date?: string;
	venue?: string;
	location?: Location;
	capacity?: number;
	budget?: { amount: number; currency: string };
	budgetTier?: string;
	setType?: string;
	setMinutes?: number;
	genres?: string[];
	notes?: string;
};

export type BookingThreadView = {
	space: string;
	skey: string;
	status: string;
	title: string | null;
	showDate: string | null;
	project: { uri: string; slug: string; name: string } | null;
	promoter: AccountView;
	participants: (AccountView & { role: 'promoter' | 'manager' })[];
	lastActivityAt: string;
	createdAt: string;
};

export type BookingEventView = {
	uri: string;
	kind: 'inquiry' | 'message' | 'status';
	author: AccountView;
	createdAt: string;
	text?: string;
	status?: string;
	show?: ShowDetails;
};

/** What an inbox item is about, which its icon shows. Email and DM opt-ins choose among these too. */
export type NotificationTopic = 'bookings' | 'guests' | 'waitlist' | 'events';

/** Where the inbox is also sent, when someone opts in. */
export type NotificationChannel = 'email' | 'dm';

/** Why a channel stopped sending, which the settings page explains. */
export type PausedReason =
	| 'address_rejected'
	| 'email_unconfirmed'
	| 'email_unverified'
	/** Bluesky chat won't deliver the root's DMs to them: they don't follow it, or allow no DMs. */
	| 'dm_not_allowed';

/**
 * Whether the root account can DM someone on Bluesky now: yes, once they follow it (their chat allows only accounts
 * they follow), not at all (it allows none), or unknown because Bluesky chat didn't answer.
 */
export type DmAvailability = 'ok' | 'needs_follow' | 'closed' | 'unavailable';

/** One person's setting for a channel, as the settings page shows it. */
export type ChannelView = {
	status: 'active' | 'paused';
	pausedReason: PausedReason | null;
	address: string | null;
	topics: NotificationTopic[];
};

/** One item in someone's inbox, rendered from what happened. `topic` is null for a kind this version doesn't know. */
export type NotificationView = {
	id: string;
	topic: NotificationTopic | null;
	title: string;
	body?: string;
	href: string;
	occurredAt: string;
	read: boolean;
};

export type PlannerSaveView = { rkey: string; project: string; note?: string; createdAt: string };

export type PlannerShowView = {
	rkey: string;
	details: ShowDetails;
	lineup: { project: string; slot?: string; status?: string }[];
	createdAt: string;
	updatedAt?: string;
};
