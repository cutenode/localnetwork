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
import { isDidString, normalizeAtprotoOauthScopeValue, SpacePermission } from '@atproto/oauth-scopes';
import { COLLECTION, ROOT_SPACE_KEY, SELF_SPACE_KEY, SPACE_TYPE } from '#lib/nsid.ts';
import type { Config } from '../config';
import { type Db, nowIso } from '../db';
import type { Logger } from '../logger';
import { AgentRepoClient, type RepoClient } from './repo-client';

function spaceScope(type: string, params: [string, string][]): string {
	return `space:${type}?${params.map(([key, value]) => `${key}=${value}`).join('&')}`;
}

/**
 * Granular OAuth scopes: write access to the user's own repos in the root
 * account's audit, moderation, and booking spaces, plus a personal planner
 * space under the user's own DID. Nothing grants reading other users' data;
 * the app reads shared spaces with credentials issued to the root authority.
 * The base fallback is plain `atproto`, since without spaces the root account
 * writes and uploads everything on the user's behalf.
 */
export function buildScopes(rootDid: string) {
	const write = (...actions: string[]) => actions.map((action): [string, string] => ['action', action]);
	const collections = (...items: string[]) => items.map((item): [string, string] => ['collection', item]);

	const scopes = [
		'atproto',
		// Project avatars, and the JSON mix reports DJs attach to their mixes as tracklists.
		'blob?accept=image/jpeg&accept=image/png&accept=image/webp&accept=application/json',
		spaceScope(SPACE_TYPE.audit, [
			['authority', rootDid],
			['skey', ROOT_SPACE_KEY],
			...collections(COLLECTION.auditEntry),
			...write('read_self', 'create')
		]),
		spaceScope(SPACE_TYPE.moderation, [
			['authority', rootDid],
			['skey', ROOT_SPACE_KEY],
			...collections(COLLECTION.request),
			...write('read_self', 'create', 'update', 'delete')
		]),
		spaceScope(SPACE_TYPE.booking, [
			['authority', rootDid],
			...collections(COLLECTION.bookingInquiry, COLLECTION.bookingMessage, COLLECTION.bookingStatus),
			...write('read_self', 'create', 'update', 'delete')
		]),
		spaceScope(SPACE_TYPE.planner, [
			['skey', SELF_SPACE_KEY],
			...collections(COLLECTION.plannerSave, COLLECTION.plannerShow),
			...write('read', 'create', 'update', 'delete'),
			['manage', 'create'],
			['manage', 'update']
		])
	];

	return { full: scopes.join(' '), base: 'atproto' };
}

/**
 * Whether `did`'s granted scope includes every scope in `requested`. The PDS grants scopes in
 * normal form with a `self` space authority resolved to the user's DID, minus any it doesn't
 * support or the user declined, so both sides are normalized that way before comparing.
 */
export function grantsAll(granted: string | undefined | null, requested: string, did: string): boolean {
	const normalize = (scope: string) => {
		const space = SpacePermission.fromString(scope);

		if (space?.isSelfAuthority && isDidString(did)) return space.withResolvedAuthority(did).toString();

		return normalizeAtprotoOauthScopeValue(scope) ?? scope;
	};

	const scopes = new Set(granted?.split(' ').map(normalize));

	return requested.split(' ').every((scope) => scopes.has(normalize(scope)));
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
	readonly scopes: { full: string; base: string };

	constructor(private readonly deps: { config: Config; db: Db; logger: Logger; rootDid: string }) {
		this.scopes = buildScopes(deps.rootDid);
		this.client = new NodeOAuthClient({
			clientMetadata: clientMetadata(deps.config, this.scopes.full),
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
	 * (it does not run the spaces alpha yet), falls back to plain `atproto`
	 * and the root account writes on the user's behalf.
	 */
	async authorize(handle: string, state: string): Promise<URL> {
		try {
			return await this.client.authorize(handle, { scope: this.scopes.full, state });
		} catch (error) {
			const message = error instanceof Error ? error.message : String(error);

			if (!/scope/i.test(message)) throw error;
			this.deps.logger.info({ handle, err: error }, 'PDS rejected spaces scopes; using base scope');

			return this.client.authorize(handle, { scope: this.scopes.base, state });
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
