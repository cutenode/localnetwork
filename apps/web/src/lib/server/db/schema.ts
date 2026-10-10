import type { Selectable } from 'kysely';
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

export interface GeocodeCacheTable {
	query: string;
	results: string;
	createdAt: string;
}

export type RequestRow = Selectable<RequestTable>;

export type AuditEntryRow = Selectable<AuditEntryTable>;

export type AccountRow = Selectable<AccountTable>;

export type BookingThreadRow = Selectable<BookingThreadTable>;
