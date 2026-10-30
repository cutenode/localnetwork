import type { Selectable } from 'kysely';
import type { SpaceKind } from '#lib/capabilities.ts';
import type { RequestKind } from '#lib/types.ts';

/**
 * Local index. Everything here except sessions and the audit outbox is derived
 * from records stored in atproto repos and spaces, and can be rebuilt by
 * re-syncing. Published projects aren't here: HappyView indexes them.
 * Column names are snake_case in SQLite and camelCase in code.
 */
export interface Database {
	kv: KvTable;
	account: AccountTable;
	webSession: WebSessionTable;
	oauthState: OAuthStoreTable;
	oauthSession: OAuthStoreTable;
	projectDetails: ProjectDetailsTable;
	request: RequestTable;
	staffRole: StaffRoleTable;
	auditEntry: AuditEntryTable;
	auditCopy: AuditCopyTable;
	spaceWatch: SpaceWatchTable;
	syncedRepo: SyncedRepoTable;
	spaceRecord: SpaceRecordTable;
	bookingThread: BookingThreadTable;
	bookingParticipant: BookingParticipantTable;
	bookingEvent: BookingEventTable;
	epkRequest: EpkRequestTable;
	epkGrant: EpkGrantTable;
	epkDraft: EpkDraftTable;
	sharedSpace: SharedSpaceTable;
	spacePerson: SpacePersonTable;
	spaceInvite: SpaceInviteTable;
	spaceInviteLink: SpaceInviteLinkTable;
	spaceJoinRequest: SpaceJoinRequestTable;
	geocodeCache: GeocodeCacheTable;
}

export interface KvTable {
	key: string;
	value: string;
}

export interface AccountTable {
	did: string;
	handle: string | null;
	displayName: string | null;
	avatarUrl: string | null;
	pdsUrl: string | null;
	spacesCapable: number;
	grantedScope: string | null;
	/** 1 when the last sign-in's grant covers shows and follows, 0 when not, null before the app asked for them. */
	publicWrites: number | null;
	createdAt: string;
	updatedAt: string;
	lastLoginAt: string | null;
}

export interface WebSessionTable {
	tokenHash: string;
	did: string;
	createdAt: string;
	expiresAt: string;
	userAgent: string | null;
}

export interface OAuthStoreTable {
	key: string;
	value: string;
	updatedAt: string;
}

export interface ProjectDetailsTable {
	projectUri: string;
	uri: string;
	cid: string;
	details: string;
	updatedAt: string;
}

export type RequestStatus = 'pending' | 'approved' | 'rejected' | 'changes_requested' | 'withdrawn';

export interface RequestTable {
	uri: string;
	cid: string;
	repoDid: string;
	requesterDid: string;
	rkey: string;
	kind: RequestKind;
	projectUri: string | null;
	status: RequestStatus;
	record: string;
	createdAt: string;
	updatedAt: string;
	decisionUri: string | null;
	decidedBy: string | null;
	decidedAt: string | null;
	decisionReason: string | null;
	resultProjectUri: string | null;
}

export interface StaffRoleTable {
	did: string;
	role: 'admin' | 'moderator';
	grantedBy: string | null;
	note: string | null;
	uri: string | null;
	cid: string | null;
	createdAt: string;
}

export type CopyStatus = 'pending' | 'written' | 'verified' | 'failed' | 'self' | 'unsupported';

export interface AuditEntryTable {
	id: string;
	seq: number;
	actorDid: string;
	actorRole: string | null;
	action: string;
	subject: string | null;
	subjectKind: string | null;
	summary: string;
	changes: string | null;
	details: string | null;
	outcome: string;
	reason: string | null;
	via: string;
	createdAt: string;
	prevHash: string;
	hash: string;
	rootStatus: CopyStatus;
	rootUri: string | null;
	rootCid: string | null;
	rootAttempts: number;
	rootError: string | null;
	actorStatus: CopyStatus;
	actorUri: string | null;
	actorCid: string | null;
	actorAttempts: number;
	actorError: string | null;
	nextAttemptAt: string | null;
}

export interface AuditCopyTable {
	uri: string;
	repoDid: string;
	rkey: string;
	cid: string;
	record: string;
	syncedAt: string;
}

export interface SpaceWatchTable {
	spaceUri: string;
	spaceType: string;
	authorityDid: string;
	skey: string;
	spaceRev: string | null;
	registrationExpiresAt: string | null;
	lastSyncedAt: string | null;
	lastError: string | null;
	createdAt: string;
}

export interface SyncedRepoTable {
	spaceUri: string;
	repoDid: string;
	pdsUrl: string;
	rev: string;
	ltHash: Uint8Array;
	commitHash: Uint8Array | null;
	lastSyncedAt: string;
	lastError: string | null;
}

export interface SpaceRecordTable {
	spaceUri: string;
	repoDid: string;
	collection: string;
	rkey: string;
	cid: string;
	value: string;
	indexedAt: string;
}

export interface BookingThreadTable {
	spaceUri: string;
	skey: string;
	projectUri: string;
	promoterDid: string;
	status: string;
	title: string | null;
	showDate: string | null;
	createdAt: string;
	updatedAt: string;
	lastActivityAt: string;
}

export interface BookingParticipantTable {
	spaceUri: string;
	did: string;
	role: 'promoter' | 'manager';
}

export interface BookingEventTable {
	uri: string;
	spaceUri: string;
	authorDid: string;
	kind: 'inquiry' | 'message' | 'status';
	record: string;
	createdAt: string;
}

export interface EpkRequestTable {
	uri: string;
	spaceUri: string;
	epkUri: string;
	requesterDid: string;
	role: string | null;
	message: string | null;
	/** What the press kit's rules said when the request came in: auto, review or none. */
	rule: string;
	status: 'pending' | 'approved' | 'declined' | 'withdrawn';
	createdAt: string;
	decidedAt: string | null;
}

export interface EpkGrantTable {
	spaceUri: string;
	subjectDid: string;
	uri: string;
	via: string;
	requestUri: string | null;
	createdAt: string;
}

export interface EpkDraftTable {
	uri: string;
	/** The project's backstage. */
	spaceUri: string;
	repoDid: string;
	rkey: string;
	/** The manager whose draft it is: the repo's owner, or who the root wrote it for. */
	authorDid: string;
	/** The press kit it changes, or the key a new press kit gets. */
	epkRkey: string;
	cid: string;
	/** The draft record as JSON. */
	record: string;
	updatedAt: string;
	/**
	 * The key the root chose for the new press kit a draft without a base publishes, kept from before the first write so
	 * a publish that failed part way finishes under the same key.
	 */
	publishedRkey: string | null;
}

/** A performer's, party's or venue's space, from its listing status record. */
export interface SharedSpaceTable {
	spaceUri: string;
	kind: SpaceKind;
	listed: number;
	createdBy: string;
	createdAt: string;
}

export interface SpacePersonTable {
	spaceUri: string;
	did: string;
	role: string;
	/** The person's capabilities that the space's kind offers, as a JSON array in catalog order. */
	capabilities: string;
	hidden: number;
	via: string;
	addedBy: string | null;
	/** The invite, invite link or request they came in through. */
	source: string | null;
	createdAt: string;
	updatedAt: string;
}

export interface SpaceInviteTable {
	spaceUri: string;
	did: string;
	role: string;
	/** A JSON array, as in space_person. */
	capabilities: string;
	invitedBy: string;
	message: string | null;
	createdAt: string;
	expiresAt: string | null;
}

export interface SpaceInviteLinkTable {
	spaceUri: string;
	rkey: string;
	tokenHash: string;
	role: string;
	/** A JSON array, as in space_person. */
	capabilities: string;
	createdBy: string;
	createdAt: string;
	expiresAt: string | null;
}

export interface SpaceJoinRequestTable {
	spaceUri: string;
	did: string;
	message: string | null;
	createdAt: string;
}

export interface GeocodeCacheTable {
	query: string;
	results: string;
	createdAt: string;
}

export type RequestRow = Selectable<RequestTable>;

export type AuditEntryRow = Selectable<AuditEntryTable>;

export type AccountRow = Selectable<AccountTable>;

export type BookingThreadRow = Selectable<BookingThreadTable>;
