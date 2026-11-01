import type { RepoClient, RootRepoClient, StoredRecord } from './atproto/repo-client';
import type { RootSpaces } from './atproto/spaces';
import type { AuditLog } from './audit/log';
import type { Db } from './db';
import type { Logger } from './logger';
import type { Notifier } from './notifications/notifier';
import type { AccountService } from './services/accounts';

/** What every domain service needs. Built once the root account is connected. */
export type Core = {
	db: Db;
	logger: Logger;
	rootDid: string;
	spaces: RootSpaces;
	/** DIDs configured as admins via ADMIN_DIDS; they cannot be demoted in-app. */
	bootstrapAdmins: string[];
	root: () => Promise<RootRepoClient>;
	userClient: (did: string) => Promise<RepoClient | null>;
	audit: AuditLog;
	accounts: AccountService;
	/** Puts an item in people's inboxes. */
	notify: Notifier['notify'];
	/** Notifies the syncer that a new space should be watched. */
	watchSpace?: (space: string) => Promise<void>;
	/** Downloads a blob referenced from a space record, using the root's space credential. */
	fetchSpaceBlob?: (space: string, repoDid: string, cid: string) => Promise<{ bytes: Uint8Array; mimeType: string }>;
	/** Reads a record from a space the root account is a member of, using its space credential. */
	readSpaceRecord?: (input: {
		space: string;
		repo: string;
		collection: string;
		rkey: string;
	}) => Promise<StoredRecord | null>;
	/** Downloads a blob from an account's public repo (at most `maxBytes`). */
	fetchBlob?: (did: string, cid: string, maxBytes: number) => Promise<{ bytes: Uint8Array; mimeType: string }>;
};

/** Thrown for problems the user can fix or should be told about (rendered as form errors). */
export class UserError extends Error {
	constructor(
		message: string,
		readonly status = 400
	) {
		super(message);
		this.name = 'UserError';
	}
}

export class ForbiddenError extends UserError {
	constructor(message = "You don't have permission to do that") {
		super(message, 403);
		this.name = 'ForbiddenError';
	}
}

/**
 * A write that needs a fresh sign-in: there's no usable OAuth session, or the PDS turned it down for a scope the
 * session wasn't granted, since it was granted before the app asked for that scope. Pages offer signing in again.
 */
export class ReauthError extends UserError {
	constructor(message: string) {
		super(message, 401);
		this.name = 'ReauthError';
	}
}

export class NotFoundError extends UserError {
	constructor(message = 'Not found') {
		super(message, 404);
		this.name = 'NotFoundError';
	}
}
