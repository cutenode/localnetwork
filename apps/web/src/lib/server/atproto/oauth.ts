import { Agent } from '@atproto/api';
import {
	buildAtprotoLoopbackClientMetadata,
	NodeOAuthClient,
	type NodeSavedSession,
	type NodeSavedState,
	type OAuthClientMetadataInput,
	type OAuthSession,
	requestLocalLock
} from '@atproto/oauth-client-node';
import { IncludeScope, isDidString, normalizeAtprotoOauthScopeValue, SpacePermission } from '@atproto/oauth-scopes';
import { PERMISSIONS } from '#lib/nsid.ts';
import type { Config } from '../config';
import { type Db, nowIso } from '../db';
import type { Logger } from '../logger';
import { PERMISSION_SET } from './lexicons';
import { AgentRepoClient, type RepoClient } from './repo-client';

const permissions = new IncludeScope(PERMISSIONS);

/**
 * Calendar events are another authority's lexicon, and a permission set can only grant collections under its own
 * NSID authority, so writing them is asked for on its own.
 */
const CALENDAR_EVENTS = 'repo:community.lexicon.calendar.event?action=create&action=update&action=delete';

/**
 * The records the app writes to an account's public repo for any account, spaces or not: shows with their calendar
 * events, and follows. A grant without them still counts as one with spaces, so adding them didn't send everyone back
 * through sign-in; the writes that need them ask for a fresh sign-in themselves.
 */
const PUBLIC_RECORDS = [
	'repo:computer.localnetwork.show?action=create&action=update&action=delete',
	'repo:computer.localnetwork.follow?action=create&action=delete',
	CALENDAR_EVENTS
].join(' ');

/**
 * Granular OAuth scopes. Local Network's permission set (lexicons/computer/localnetwork/permissions.json)
 * grants write access to the user's own repos in the audit, moderation, and booking spaces, a
 * personal planner space under the user's own DID, and the records it writes to the user's public
 * repo (press kits, shows, follows). The set leaves the shared spaces' authority open, since one
 * published document serves every network and each has its own root DID. Blob uploads and calendar
 * events stay explicit: a permission set can only hold repo, rpc, and space permissions, and only
 * for its own namespace. Nothing grants reading other users' data; the app reads shared spaces with
 * credentials issued to the root authority. The base fallback, for a PDS without spaces, asks for
 * the public records anyone can write (follows, and shows with their events); the root account
 * writes and uploads everything else on the user's behalf.
 */
export const SCOPES = {
	full: [
		'atproto',
		// Project avatars, and the JSON mix reports DJs attach to their mixes as tracklists.
		'blob?accept=image/jpeg&accept=image/png&accept=image/webp&accept=application/json',
		CALENDAR_EVENTS,
		permissions.toString()
	].join(' '),
	base: `atproto ${PUBLIC_RECORDS}`,
	/** For a PDS that refuses even the base scopes' granular permissions. */
	minimal: 'atproto'
};

/** Every scope the app may ask for. A PDS refuses an authorization for any scope its client metadata doesn't list. */
export const DECLARED_SCOPES = [...new Set(Object.values(SCOPES).flatMap((scope) => scope.split(' ')))].join(' ');

/**
 * Whether `did`'s granted scope includes every scope in `requested`. The PDS grants scopes in
 * normal form, with the permission set expanded into the scopes it lists and a `self` space
 * authority resolved to the user's DID, minus any it doesn't support or the user declined, so
 * both sides are expanded and normalized that way before comparing.
 */
export function grantsAll(
	granted: string | undefined | null,
	requested: string,
	did: string,
	except: string[] = []
): boolean {
	const normalize = (scope: string): string[] => {
		if (scope === permissions.toString()) return permissions.toScopes(PERMISSION_SET).flatMap(normalize);

		const space = SpacePermission.fromString(scope);

		if (space?.isSelfAuthority && isDidString(did)) return [space.withResolvedAuthority(did).toString()];

		return [normalizeAtprotoOauthScopeValue(scope) ?? scope];
	};

	const scopes = new Set(granted?.split(' ').flatMap(normalize));
	const skipped = new Set(except.flatMap(normalize));

	return requested
		.split(' ')
		.flatMap(normalize)
		.every((scope) => scopes.has(scope) || skipped.has(scope));
}

/**
 * Whether a grant lets the app make the space writes it makes with a user's session, which is what makes an account
 * spaces-capable. The public records aren't needed for that (see PUBLIC_RECORDS).
 */
export function grantsSpaces(granted: string | undefined | null, did: string): boolean {
	return grantsAll(granted, SCOPES.full, did, PUBLIC_RECORDS.split(' '));
}

/** Whether a grant lets the app write shows, calendar events, and follows to the account's public repo. */
export function grantsPublicRecords(granted: string | undefined | null, did: string): boolean {
	return grantsAll(granted, PUBLIC_RECORDS, did);
}

export function isLoopback(publicUrl: string): boolean {
	const { hostname } = new URL(publicUrl);

	return hostname === '127.0.0.1' || hostname === '[::1]' || hostname === 'localhost';
}

export function clientMetadata(config: Config, scope: string): OAuthClientMetadataInput {
	const redirectUri = `${config.publicUrl}/oauth/callback` as const;

	if (isLoopback(config.publicUrl)) {
		return buildAtprotoLoopbackClientMetadata({
			redirect_uris: [redirectUri],
			scope
		});
	}

	return {
		client_id: `${config.publicUrl}/oauth-client-metadata.json`,
		client_name: 'Local Network',
		client_uri: config.publicUrl,
		tos_uri: `${config.publicUrl}/about`,
		policy_uri: `${config.publicUrl}/privacy`,
		redirect_uris: [redirectUri],
		scope,
		grant_types: ['authorization_code', 'refresh_token'],
		response_types: ['code'],
		token_endpoint_auth_method: 'none',
		application_type: 'web',
		dpop_bound_access_tokens: true
	};
}

function sqliteStore<T>(db: Db, table: 'oauthState' | 'oauthSession') {
	return {
		async get(key: string): Promise<T | undefined> {
			const row = await db.selectFrom(table).select('value').where('key', '=', key).executeTakeFirst();

			// SAFETY: `set` below is the only writer of these rows, and it stores `JSON.stringify` of a T (the
			// client's saved state or session, both plain JSON data).
			return row ? (JSON.parse(row.value) as T) : undefined;
		},
		async set(key: string, value: T): Promise<void> {
			const json = JSON.stringify(value);
			await db
				.insertInto(table)
				.values({ key, value: json, updatedAt: nowIso() })
				.onConflict((oc) => oc.column('key').doUpdateSet({ value: json, updatedAt: nowIso() }))
				.execute();
		},
		async del(key: string): Promise<void> {
			await db.deleteFrom(table).where('key', '=', key).execute();
		}
	};
}

export class OAuthService {
	readonly client: NodeOAuthClient;
	readonly scopes = SCOPES;

	constructor(private readonly deps: { config: Config; db: Db; logger: Logger }) {
		this.client = new NodeOAuthClient({
			clientMetadata: clientMetadata(deps.config, DECLARED_SCOPES),
			allowHttp: deps.config.dev,
			plcDirectoryUrl: deps.config.plcUrl,
			handleResolver: deps.config.handleResolverUrl,
			requestLock: requestLocalLock,
			stateStore: sqliteStore<NodeSavedState>(deps.db, 'oauthState'),
			sessionStore: sqliteStore<NodeSavedSession>(deps.db, 'oauthSession')
		});
	}

	get metadata() {
		return this.client.clientMetadata;
	}

	/**
	 * Starts an authorization. If the user's PDS rejects the spaces scopes
	 * (it does not run the spaces alpha yet), falls back to the base scopes
	 * and the root account writes on the user's behalf, then to plain
	 * `atproto` for a PDS that doesn't know granular scopes at all.
	 */
	async authorize(handle: string, state: string): Promise<URL> {
		const [first, ...fallbacks] = [this.scopes.full, this.scopes.base, this.scopes.minimal];
		let scope = first;

		for (;;) {
			try {
				return await this.client.authorize(handle, { scope, state });
			} catch (error) {
				const message = error instanceof Error ? error.message : String(error);
				const next = fallbacks.shift();

				if (!next || !/scope/i.test(message)) throw error;
				this.deps.logger.info({ handle, err: error, scope: next }, 'PDS rejected scopes; asking for fewer');
				scope = next;
			}
		}
	}

	async callback(params: URLSearchParams) {
		const { session, state } = await this.client.callback(params);
		const info = await session.getTokenInfo(false);

		return { session, state, scope: info.scope };
	}

	async restore(did: string): Promise<OAuthSession | null> {
		try {
			return await this.client.restore(did);
		} catch (error) {
			this.deps.logger.debug({ did, err: error }, 'no usable OAuth session');

			return null;
		}
	}

	async repoClient(did: string): Promise<RepoClient | null> {
		const session = await this.restore(did);

		return session ? new AgentRepoClient(did, new Agent(session)) : null;
	}

	async revoke(did: string): Promise<void> {
		try {
			await this.client.revoke(did);
		} catch (error) {
			this.deps.logger.warn({ did, err: error }, 'could not revoke OAuth session');
		}
	}
}
