import { type Kysely, sql } from 'kysely';
import { type Migration, type MigrationProvider, Migrator } from 'kysely/migration';
import type { Database } from './schema';

const statements = (...items: string[]) => items;

const init = statements(
	`CREATE TABLE kv (key TEXT PRIMARY KEY, value TEXT NOT NULL)`,
	`CREATE TABLE account (
		did TEXT PRIMARY KEY,
		handle TEXT,
		display_name TEXT,
		avatar_url TEXT,
		pds_url TEXT,
		spaces_capable INTEGER NOT NULL DEFAULT 0,
		granted_scope TEXT,
		created_at TEXT NOT NULL,
		updated_at TEXT NOT NULL,
		last_login_at TEXT
	)`,
	`CREATE INDEX account_handle ON account (handle)`,
	`CREATE TABLE web_session (
		token_hash TEXT PRIMARY KEY,
		did TEXT NOT NULL,
		created_at TEXT NOT NULL,
		expires_at TEXT NOT NULL,
		user_agent TEXT
	)`,
	`CREATE INDEX web_session_did ON web_session (did)`,
	`CREATE TABLE oauth_state (key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at TEXT NOT NULL)`,
	`CREATE TABLE oauth_session (key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at TEXT NOT NULL)`,
	`CREATE TABLE project_details (
		project_uri TEXT PRIMARY KEY,
		uri TEXT NOT NULL,
		cid TEXT NOT NULL,
		details TEXT NOT NULL,
		updated_at TEXT NOT NULL
	)`,
	`CREATE TABLE request (
		uri TEXT PRIMARY KEY,
		cid TEXT NOT NULL,
		repo_did TEXT NOT NULL,
		requester_did TEXT NOT NULL,
		rkey TEXT NOT NULL,
		kind TEXT NOT NULL,
		project_uri TEXT,
		status TEXT NOT NULL,
		record TEXT NOT NULL,
		created_at TEXT NOT NULL,
		updated_at TEXT NOT NULL,
		decision_uri TEXT,
		decided_by TEXT,
		decided_at TEXT,
		decision_reason TEXT,
		result_project_uri TEXT
	)`,
	`CREATE INDEX request_status ON request (status, created_at)`,
	`CREATE INDEX request_requester ON request (requester_did, created_at)`,
	`CREATE INDEX request_project ON request (project_uri)`,
	`CREATE TABLE staff_role (
		did TEXT PRIMARY KEY,
		role TEXT NOT NULL,
		granted_by TEXT,
		note TEXT,
		uri TEXT,
		cid TEXT,
		created_at TEXT NOT NULL
	)`,
	`CREATE TABLE audit_entry (
		id TEXT PRIMARY KEY,
		seq INTEGER NOT NULL UNIQUE,
		actor_did TEXT NOT NULL,
		actor_role TEXT,
		action TEXT NOT NULL,
		subject TEXT,
		subject_kind TEXT,
		summary TEXT NOT NULL,
		changes TEXT,
		details TEXT,
		outcome TEXT NOT NULL,
		reason TEXT,
		via TEXT NOT NULL,
		created_at TEXT NOT NULL,
		prev_hash TEXT NOT NULL,
		hash TEXT NOT NULL,
		root_status TEXT NOT NULL,
		root_uri TEXT,
		root_cid TEXT,
		root_attempts INTEGER NOT NULL DEFAULT 0,
		root_error TEXT,
		actor_status TEXT NOT NULL,
		actor_uri TEXT,
		actor_cid TEXT,
		actor_attempts INTEGER NOT NULL DEFAULT 0,
		actor_error TEXT,
		next_attempt_at TEXT
	)`,
	`CREATE INDEX audit_entry_actor ON audit_entry (actor_did, seq)`,
	`CREATE INDEX audit_entry_action ON audit_entry (action, seq)`,
	`CREATE INDEX audit_entry_subject ON audit_entry (subject)`,
	`CREATE INDEX audit_entry_pending ON audit_entry (next_attempt_at) WHERE next_attempt_at IS NOT NULL`,
	`CREATE TABLE audit_copy (
		uri TEXT PRIMARY KEY,
		repo_did TEXT NOT NULL,
		rkey TEXT NOT NULL,
		cid TEXT NOT NULL,
		record TEXT NOT NULL,
		synced_at TEXT NOT NULL
	)`,
	`CREATE INDEX audit_copy_rkey ON audit_copy (rkey)`,
	`CREATE TABLE space_watch (
		space_uri TEXT PRIMARY KEY,
		space_type TEXT NOT NULL,
		authority_did TEXT NOT NULL,
		skey TEXT NOT NULL,
		space_rev TEXT,
		registration_expires_at TEXT,
		last_synced_at TEXT,
		last_error TEXT,
		created_at TEXT NOT NULL
	)`,
	`CREATE TABLE synced_repo (
		space_uri TEXT NOT NULL,
		repo_did TEXT NOT NULL,
		pds_url TEXT NOT NULL,
		rev TEXT NOT NULL,
		lt_hash BLOB NOT NULL,
		commit_hash BLOB,
		last_synced_at TEXT NOT NULL,
		last_error TEXT,
		PRIMARY KEY (space_uri, repo_did)
	)`,
	`CREATE TABLE space_record (
		space_uri TEXT NOT NULL,
		repo_did TEXT NOT NULL,
		collection TEXT NOT NULL,
		rkey TEXT NOT NULL,
		cid TEXT NOT NULL,
		value TEXT NOT NULL,
		indexed_at TEXT NOT NULL,
		PRIMARY KEY (space_uri, repo_did, collection, rkey)
	)`,
	`CREATE TABLE booking_thread (
		space_uri TEXT PRIMARY KEY,
		skey TEXT NOT NULL,
		project_uri TEXT NOT NULL,
		promoter_did TEXT NOT NULL,
		status TEXT NOT NULL,
		title TEXT,
		show_date TEXT,
		created_at TEXT NOT NULL,
		updated_at TEXT NOT NULL,
		last_activity_at TEXT NOT NULL
	)`,
	`CREATE INDEX booking_thread_project ON booking_thread (project_uri)`,
	`CREATE TABLE booking_participant (
		space_uri TEXT NOT NULL REFERENCES booking_thread (space_uri) ON DELETE CASCADE,
		did TEXT NOT NULL,
		role TEXT NOT NULL,
		PRIMARY KEY (space_uri, did)
	)`,
	`CREATE INDEX booking_participant_did ON booking_participant (did)`,
	`CREATE TABLE booking_event (
		uri TEXT PRIMARY KEY,
		space_uri TEXT NOT NULL,
		author_did TEXT NOT NULL,
		kind TEXT NOT NULL,
		record TEXT NOT NULL,
		created_at TEXT NOT NULL
	)`,
	`CREATE INDEX booking_event_space ON booking_event (space_uri, created_at)`,
	`CREATE TABLE geocode_cache (query TEXT PRIMARY KEY, results TEXT NOT NULL, created_at TEXT NOT NULL)`
);

/** Press kit access: requests people make and grants owners write, as both sides' PDSes hold them. */
const epk = statements(
	`CREATE TABLE epk_request (
		uri TEXT PRIMARY KEY,
		space_uri TEXT NOT NULL,
		epk_uri TEXT NOT NULL,
		requester_did TEXT NOT NULL,
		role TEXT,
		message TEXT,
		rule TEXT NOT NULL,
		status TEXT NOT NULL,
		created_at TEXT NOT NULL,
		decided_at TEXT
	)`,
	`CREATE INDEX epk_request_space ON epk_request (space_uri, status)`,
	`CREATE INDEX epk_request_requester ON epk_request (requester_did, space_uri)`,
	`CREATE TABLE epk_grant (
		space_uri TEXT NOT NULL,
		subject_did TEXT NOT NULL,
		uri TEXT NOT NULL,
		via TEXT NOT NULL,
		request_uri TEXT,
		created_at TEXT NOT NULL,
		PRIMARY KEY (space_uri, subject_did)
	)`
);

/** Press kit drafts in projects' backstages, from the managers' repos (or the root's, on their behalf). */
const epkDrafts = statements(
	`CREATE TABLE epk_draft (
		uri TEXT PRIMARY KEY,
		space_uri TEXT NOT NULL,
		repo_did TEXT NOT NULL,
		rkey TEXT NOT NULL,
		author_did TEXT NOT NULL,
		epk_rkey TEXT NOT NULL,
		cid TEXT NOT NULL,
		record TEXT NOT NULL,
		updated_at TEXT NOT NULL,
		published_rkey TEXT
	)`,
	`CREATE INDEX epk_draft_space ON epk_draft (space_uri, epk_rkey)`
);

/**
 * People's inboxes (notifications/notifier.ts), which nothing on the network holds a copy of; how each person wants
 * them sent on by email or DM (notifications/channels.ts); and the outbox of what's waiting to go
 * (notifications/outbox.ts).
 */
const inbox = statements(
	`CREATE TABLE notification (
		id TEXT PRIMARY KEY,
		recipient_did TEXT NOT NULL,
		kind TEXT NOT NULL,
		dedupe_key TEXT NOT NULL,
		subject_uri TEXT NOT NULL,
		href TEXT NOT NULL,
		data TEXT NOT NULL,
		occurred_at TEXT NOT NULL,
		created_at TEXT NOT NULL,
		read_at TEXT,
		UNIQUE (recipient_did, dedupe_key)
	)`,
	`CREATE INDEX notification_recipient ON notification (recipient_did, occurred_at DESC, id DESC)`,
	// The unread badge counts these on every dashboard page.
	`CREATE INDEX notification_unread ON notification (recipient_did) WHERE read_at IS NULL`,
	`CREATE TABLE notification_channel (
		did TEXT NOT NULL,
		channel TEXT NOT NULL,
		status TEXT NOT NULL,
		paused_reason TEXT,
		address TEXT,
		address_read_at TEXT,
		unsubscribe_token TEXT UNIQUE,
		topics TEXT NOT NULL,
		last_sent_at TEXT,
		created_at TEXT NOT NULL,
		updated_at TEXT NOT NULL,
		confirm_token_hash TEXT,
		confirm_address_hash TEXT,
		confirm_expires_at TEXT,
		PRIMARY KEY (did, channel)
	)`,
	`CREATE TABLE notification_delivery (
		id TEXT PRIMARY KEY,
		notification_id TEXT NOT NULL REFERENCES notification (id) ON DELETE CASCADE,
		did TEXT NOT NULL,
		channel TEXT NOT NULL,
		status TEXT NOT NULL,
		batch_id TEXT,
		attempts INTEGER NOT NULL DEFAULT 0,
		next_attempt_at TEXT NOT NULL,
		last_error TEXT,
		provider_id TEXT,
		sent_at TEXT,
		updated_at TEXT NOT NULL,
		UNIQUE (notification_id, channel)
	)`,
	`CREATE INDEX notification_delivery_due ON notification_delivery (status, next_attempt_at)`
);

const migrations = {
	'001_init': {
		async up(db) {
			for (const statement of init) await sql.raw(statement).execute(db);
		}
	},
	'002_epk': {
		async up(db) {
			for (const statement of epk) await sql.raw(statement).execute(db);
		}
	},
	// Whether the last sign-in's grant covers shows and follows; null for sign-ins from before the app asked.
	'003_public_writes': {
		async up(db) {
			await sql.raw('ALTER TABLE account ADD COLUMN public_writes INTEGER').execute(db);
		}
	},
	'004_epk_drafts': {
		async up(db) {
			for (const statement of epkDrafts) await sql.raw(statement).execute(db);
		}
	},
	// The set script's re-read version now sits under a key per collection (happyview/refresh.ts).
	'005_refresh_keys': {
		async up(db) {
			await sql.raw("DELETE FROM kv WHERE key = 'happyview_set_script_refreshed'").execute(db);
		}
	},
	'007_inbox': {
		async up(db) {
			for (const statement of inbox) await sql.raw(statement).execute(db);
		}
	}
} satisfies Record<string, Migration>;

class StaticProvider implements MigrationProvider {
	async getMigrations() {
		return migrations;
	}
}

export async function migrateToLatest(db: Kysely<Database>): Promise<void> {
	const migrator = new Migrator({ db, provider: new StaticProvider() });
	const { error, results } = await migrator.migrateToLatest();
	const failed = results?.find((result) => result.status === 'Error');

	if (error || failed) {
		throw new Error(`Database migration failed${failed ? ` at ${failed.migrationName}` : ''}`, {
			cause: error
		});
	}
}
