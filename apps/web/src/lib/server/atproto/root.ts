import { scrypt } from 'node:crypto';
import { promisify } from 'node:util';
import { AtpAgent, type AtpSessionData } from '@atproto/api';
import type { Db } from '../db';
import type { Logger } from '../logger';
import type { IdentityResolver } from './identity';
import { AgentRootClient, type RootRepoClient } from './repo-client';

const SESSION_KEY = 'root_session';

const scryptAsync = promisify<string, string, number, Buffer>(scrypt);

export type RootAccountOptions = {
	identifier: string;
	password: string;
	service?: string;
	db: Db;
	identity: IdentityResolver;
	logger: Logger;
};

/**
 * The Local Network root account. The app signs in with the account password
 * (legacy auth carries full permissions on the PDS, which space management
 * and writing on behalf of the platform require) and persists the session so
 * restarts do not burn createSession rate limits.
 */
export class RootAccount {
	private agent: AtpAgent | undefined;
	private client: RootRepoClient | undefined;
	private connecting: Promise<RootRepoClient> | undefined;
	did: string | undefined;
	handle: string | undefined;
	service: string | undefined;

	constructor(private readonly options: RootAccountOptions) {}

	async connect(): Promise<RootRepoClient> {
		if (this.client) return this.client;
		this.connecting ??= this.login().finally(() => {
			this.connecting = undefined;
		});

		return this.connecting;
	}

	/** The root account's DID; throws if `connect` has not completed. */
	get rootDid(): string {
		if (!this.did) throw new Error('Root account is not connected');

		return this.did;
	}

	private async login(): Promise<RootRepoClient> {
		const { identity, logger, db } = this.options;
		const did = await identity.resolveIdentifier(this.options.identifier);
		// Bind the stored session to the configured password so changing
		// ROOT_PASSWORD (e.g. from an app password) forces a fresh sign-in.
		// The fingerprint sits in the database, so it's a slow hash: a copy of the
		// database shouldn't make the root password cheap to guess.
		const fingerprint = (await scryptAsync(this.options.password, did, 32)).toString('hex');
		const service = this.options.service ?? (await identity.resolve(did)).pds;

		const agent = new AtpAgent({
			service,
			persistSession: async (event, session) => {
				if (session && (event === 'create' || event === 'update')) {
					const value = JSON.stringify({ fingerprint, session });
					await db
						.insertInto('kv')
						.values({ key: SESSION_KEY, value })
						.onConflict((oc) => oc.column('key').doUpdateSet({ value }))
						.execute();
				} else if (event === 'expired' || event === 'create-failed') {
					await db.deleteFrom('kv').where('key', '=', SESSION_KEY).execute();
				}
			}
		});

		const saved = await db.selectFrom('kv').select('value').where('key', '=', SESSION_KEY).executeTakeFirst();
		let resumed = false;

		if (saved) {
			try {
				// SAFETY: `persistSession` above is the only writer of this key, and it stores
				// `JSON.stringify({ fingerprint, session })`.
				const stored = JSON.parse(saved.value) as { fingerprint?: string; session?: AtpSessionData };
				const session = stored.session;

				if (session && stored.fingerprint === fingerprint && session.did === did) {
					await agent.resumeSession(session);
					resumed = true;
				}
			} catch (error) {
				logger.warn({ err: error }, 'could not resume root session; signing in again');
			}
		}

		if (!resumed) {
			await agent.login({ identifier: did, password: this.options.password });
		}

		if (agent.did !== did) throw new Error(`Root session DID ${agent.did} does not match ${did}`);

		this.agent = agent;
		this.did = did;
		this.handle = agent.session?.handle;
		this.service = service;
		this.client = new AgentRootClient(did, agent);
		logger.info({ did, handle: this.handle, service, resumed }, 'root account connected');

		return this.client;
	}
}
