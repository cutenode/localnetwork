import type { AuditEntryRow } from '#lib/server/db/schema.ts';
import type { Viewer } from '#lib/types.ts';

/** A made-up entry in someone's activity. */
export const auditEntry = (seq: number, overrides: Partial<AuditEntryRow> = {}): AuditEntryRow => ({
	id: String(seq),
	seq,
	actorDid: 'did:plc:neon',
	actorRole: 'member',
	action: 'follow.create',
	subject: null,
	subjectKind: null,
	summary: 'Followed Neon Tide',
	changes: null,
	details: null,
	outcome: 'success',
	reason: null,
	via: 'web',
	createdAt: '2026-10-30T20:00:00Z',
	prevHash: 'a',
	hash: 'b',
	rootStatus: 'verified',
	rootUri: null,
	rootCid: null,
	rootAttempts: 0,
	rootError: null,
	actorStatus: 'verified',
	actorUri: null,
	actorCid: null,
	actorAttempts: 0,
	actorError: null,
	nextAttemptAt: null,
	...overrides
});

const accountViewer: Viewer = {
	did: 'did:plc:neon',
	handle: 'neon.test',
	roles: [],
	isAdmin: false,
	isStaff: false,
	spacesCapable: true
};

/** What the layouts above an Account page load, for its fixtures. */
export const accountLayout = {
	viewer: accountViewer,
	pendingRequests: 0,
	unread: 0,
	waiting: 0,
	needs: [],
	access: Promise.resolve([]),
	projects: Promise.resolve([]),
	sections: Promise.resolve(null)
};
