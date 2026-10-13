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
	epk: `${NS}.epk`,
	epkContent: `${NS}.epk.content`,
	epkRequest: `${NS}.epk.request`,
	epkGrant: `${NS}.epk.grant`
} as const;

/** Queries served by the directory, the HappyView AppView that indexes published projects. */
export const DIRECTORY = {
	searchProjects: `${NS}.directory.searchProjects`,
	matchProjects: `${NS}.directory.matchProjects`,
	getProject: `${NS}.directory.getProject`,
	getProjects: `${NS}.directory.getProjects`,
	getStats: `${NS}.directory.getStats`
} as const;

export const SPACE_TYPE = {
	audit: `${NS}.space.audit`,
	moderation: `${NS}.space.moderation`,
	staff: `${NS}.space.staff`,
	industry: `${NS}.space.industry`,
	booking: `${NS}.space.booking`,
	planner: `${NS}.space.planner`,
	epk: `${NS}.space.epk`
} as const;

export type SpaceType = (typeof SPACE_TYPE)[keyof typeof SPACE_TYPE];

/** Space keys for the singleton spaces anchored on the root account. */
export const ROOT_SPACE_KEY = 'main' as const;

/** The private part of a press kit's single record key in its space. */
export const EPK_CONTENT_KEY = 'self' as const;

/** Space key for personal spaces anchored on a user's own DID. */
export const SELF_SPACE_KEY = 'self' as const;
