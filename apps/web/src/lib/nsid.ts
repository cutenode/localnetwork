/**
 * Lexicon NSIDs used by Local Network. The namespace is `computer.localnetwork`,
 * the reverse of the app's domain, localnetwork.computer.
 */
export const NS = 'computer.localnetwork' as const;

export const COLLECTION = {
	project: `${NS}.project`,
	projectDetails: `${NS}.project.details`,
	auditEntry: `${NS}.audit.entry`,
	request: `${NS}.moderation.request`,
	decision: `${NS}.moderation.decision`,
	staffRole: `${NS}.staff.role`,
	bookingInquiry: `${NS}.booking.inquiry`,
	bookingMessage: `${NS}.booking.message`,
	bookingStatus: `${NS}.booking.status`,
	plannerSave: `${NS}.planner.save`,
	plannerShow: `${NS}.planner.show`,
	/** A DJ's playing stats, written by locallibrary to the DJ's own repo. */
	djProfile: `${NS}.dj.profile`,
	/** A DJ's set, written by locallibrary to the DJ's own repo. */
	djSet: `${NS}.dj.set`,
	/** A DJ set's recording, uploaded by locallibrary to the DJ's own repo. */
	djRecording: `${NS}.dj.recording`,
	/** A DJ's rekordbox settings, written by locallibrary to the DJ's own repo. */
	locallibrarySettings: `${NS}.locallibrary.settings`,
	epk: `${NS}.epk`,
	epkContent: `${NS}.epk.content`,
	epkRequest: `${NS}.epk.request`,
	epkGrant: `${NS}.epk.grant`,
	/** A manager's unpublished changes to a press kit, in their repo in the project's backstage. */
	epkDraft: `${NS}.epk.draft`,
	/** A show a manager announced, in their own repo beside its calendar event. */
	show: `${NS}.show`,
	follow: `${NS}.follow`,
	/** Someone in a performer's, party's or venue's space, with their role and capabilities. */
	person: `${NS}.people.person`,
	invite: `${NS}.people.invite`,
	inviteLink: `${NS}.people.link`,
	joinRequest: `${NS}.people.request`,
	/** Whether staff list a performer, party or venue in the directory, in its space. */
	listingStatus: `${NS}.listing.status`,
	/** A listed act's, party's or venue's directory listing, in the root's public repo. */
	listing: `${NS}.listing`
} as const;

/** The lexicon-community calendar event a show is, so other calendar apps can read it. */
export const CALENDAR_EVENT = 'community.lexicon.calendar.event' as const;

/** Bluesky's verification, which the root account issues to vouch that an account is who it says it is. */
export const VERIFICATION = 'app.bsky.graph.verification' as const;

/** Bluesky's profile record, where an account's display name lives. */
export const BSKY_PROFILE = 'app.bsky.actor.profile' as const;

/** Bluesky's follow record, which press kits' "people you follow" audience reads. */
export const BSKY_FOLLOW = 'app.bsky.graph.follow' as const;

/**
 * Queries served by the directory, the HappyView AppView that indexes published projects, their managers' DJ records,
 * press kits and shows, and follows.
 */
export const DIRECTORY = {
	searchProjects: `${NS}.directory.searchProjects`,
	matchProjects: `${NS}.directory.matchProjects`,
	getProject: `${NS}.directory.getProject`,
	getProjects: `${NS}.directory.getProjects`,
	getStats: `${NS}.directory.getStats`,
	getSets: `${NS}.directory.getSets`,
	listSets: `${NS}.directory.listSets`,
	listShows: `${NS}.directory.listShows`,
	getShows: `${NS}.directory.getShows`,
	getFollowStatus: `${NS}.directory.getFollowStatus`,
	getCharts: `${NS}.directory.getCharts`,
	getArtist: `${NS}.directory.getArtist`,
	searchArtists: `${NS}.directory.searchArtists`,
	getRecords: `${NS}.directory.getRecords`,
	listRecords: `${NS}.directory.listRecords`
} as const;

/**
 * The directory procedure this app answers itself rather than HappyView (it needs the app's HappyView API key): has
 * HappyView read a caller's locallibrary records again.
 */
export const REQUEST_INDEX = `${NS}.directory.requestIndex` as const;

export const SPACE_TYPE = {
	audit: `${NS}.space.audit`,
	moderation: `${NS}.space.moderation`,
	staff: `${NS}.space.staff`,
	industry: `${NS}.space.industry`,
	booking: `${NS}.space.booking`,
	planner: `${NS}.space.planner`,
	epk: `${NS}.space.epk`,
	backstage: `${NS}.space.backstage`,
	performer: `${NS}.space.performer`,
	party: `${NS}.space.party`,
	venue: `${NS}.space.venue`
} as const;

export type SpaceType = (typeof SPACE_TYPE)[keyof typeof SPACE_TYPE];

/** Space keys for the singleton spaces anchored on the root account. */
export const ROOT_SPACE_KEY = 'main' as const;

/** The private part of a press kit's single record key in its space. */
export const EPK_CONTENT_KEY = 'self' as const;

/** Space key for personal spaces anchored on a user's own DID. */
export const SELF_SPACE_KEY = 'self' as const;

/** The OAuth permission set (`include:`) the app requests: its space writes, in one consent line. */
export const PERMISSIONS = `${NS}.permissions` as const;
