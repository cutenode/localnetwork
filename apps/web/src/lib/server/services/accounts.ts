import type { AccountView } from '#lib/types.ts';
import type { IdentityResolver } from '../atproto/identity';
import { type Db, nowIso } from '../db';
import type { AccountRow } from '../db/schema';
import type { Logger } from '../logger';

export class AccountService {
	constructor(
		private readonly db: Db,
		private readonly logger: Logger,
		private readonly identity?: IdentityResolver
	) {}

	async recordLogin(input: {
		did: string;
		handle: string | null;
		pdsUrl: string | null;
		grantedScope: string;
		spacesCapable: boolean;
		/** Whether the grant lets the app write shows and follows to the account's public repo. */
		publicWrites: boolean;
	}) {
		const now = nowIso();
		await this.db
			.insertInto('account')
			.values({
				did: input.did,
				handle: input.handle,
				displayName: null,
				avatarUrl: null,
				pdsUrl: input.pdsUrl,
				spacesCapable: input.spacesCapable ? 1 : 0,
				grantedScope: input.grantedScope,
				publicWrites: input.publicWrites ? 1 : 0,
				createdAt: now,
				updatedAt: now,
				lastLoginAt: now
			})
			.onConflict((oc) =>
				oc.column('did').doUpdateSet({
					handle: input.handle,
					pdsUrl: input.pdsUrl,
					spacesCapable: input.spacesCapable ? 1 : 0,
					grantedScope: input.grantedScope,
					publicWrites: input.publicWrites ? 1 : 0,
					updatedAt: now,
					lastLoginAt: now
				})
			)
			.execute();
	}

	get(did: string): Promise<AccountRow | undefined> {
		return this.db.selectFrom('account').selectAll().where('did', '=', did).executeTakeFirst();
	}

	async spacesCapable(did: string): Promise<boolean> {
		return (await this.get(did))?.spacesCapable === 1;
	}

	/** Handles for a set of DIDs from the local cache, resolving unknown ones in the background. */
	async views(dids: Iterable<string>): Promise<Map<string, AccountView>> {
		const unique = [...new Set(dids)].filter(Boolean);
		const map = new Map<string, AccountView>();

		if (unique.length === 0) return map;
		const rows = await this.db.selectFrom('account').select(['did', 'handle']).where('did', 'in', unique).execute();

		for (const row of rows) map.set(row.did, { did: row.did, handle: row.handle });
		const missing = unique.filter((did) => !map.has(did) || !map.get(did)?.handle);

		for (const did of unique) if (!map.has(did)) map.set(did, { did, handle: null });

		if (missing.length > 0) void this.learn(missing);

		return map;
	}

	async view(did: string): Promise<AccountView> {
		return (await this.views([did])).get(did) ?? { did, handle: null };
	}

	/** Resolves and caches handles for DIDs the app has not seen sign in. */
	async learn(dids: string[]): Promise<void> {
		if (!this.identity) return;

		for (const did of dids.slice(0, 25)) {
			try {
				const resolved = await this.identity.resolve(did);
				const now = nowIso();
				await this.db
					.insertInto('account')
					.values({
						did,
						handle: resolved.handle,
						displayName: null,
						avatarUrl: null,
						pdsUrl: resolved.pds,
						spacesCapable: 0,
						grantedScope: null,
						publicWrites: null,
						createdAt: now,
						updatedAt: now,
						lastLoginAt: null
					})
					.onConflict((oc) =>
						oc.column('did').doUpdateSet({ handle: resolved.handle, pdsUrl: resolved.pds, updatedAt: now })
					)
					.execute();
			} catch (error) {
				this.logger.debug({ err: error, did }, 'could not resolve account');
			}
		}
	}
}
