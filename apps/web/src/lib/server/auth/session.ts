import { createHash, randomBytes } from 'node:crypto';
import { type Db, nowIso } from '../db';

export const SESSION_COOKIE = 'ln_session';

/** The shape of a token from {@link newToken}: 32 random bytes in base64url. */
export const TOKEN_PATTERN = /^[A-Za-z0-9_-]{43}$/;

/** A secret token, like a session's or an invite link's, which only ever gets stored as its {@link hashToken}. */
export const newToken = () => randomBytes(32).toString('base64url');

export const hashToken = (token: string) => createHash('sha256').update(token).digest('base64url');

/**
 * Opaque browser sessions. Only a SHA-256 of the token is stored, so a leaked
 * database cannot be replayed as cookies.
 */
export class WebSessions {
	constructor(
		private readonly db: Db,
		private readonly days: number
	) {}

	get maxAgeSeconds(): number {
		return this.days * 86_400;
	}

	async create(did: string, userAgent: string | null): Promise<string> {
		const token = newToken();
		const now = new Date();
		await this.db.deleteFrom('webSession').where('expiresAt', '<=', now.toISOString()).execute();
		await this.db
			.insertInto('webSession')
			.values({
				tokenHash: hashToken(token),
				did,
				createdAt: now.toISOString(),
				expiresAt: new Date(now.getTime() + this.maxAgeSeconds * 1000).toISOString(),
				userAgent: userAgent?.slice(0, 300) ?? null
			})
			.execute();

		return token;
	}

	async resolve(token: string | undefined): Promise<string | null> {
		if (!token || !TOKEN_PATTERN.test(token)) return null;

		const row = await this.db
			.selectFrom('webSession')
			.select('did')
			.where('tokenHash', '=', hashToken(token))
			.where('expiresAt', '>', nowIso())
			.executeTakeFirst();

		return row?.did ?? null;
	}

	async destroy(token: string | undefined): Promise<void> {
		if (!token || !TOKEN_PATTERN.test(token)) return;
		await this.db.deleteFrom('webSession').where('tokenHash', '=', hashToken(token)).execute();
	}

	async destroyAll(did: string): Promise<void> {
		await this.db.deleteFrom('webSession').where('did', '=', did).execute();
	}
}
