import { z } from 'zod';
import type { ActorSuggestion } from '#lib/actors.ts';
import type { Logger } from './logger';

type Provider = 'bluesky' | 'none';

// Bluesky's public AppView answers prefix searches over every account it has seen. HappyView only indexes the
// accounts this site lists, so it can't suggest anyone else's handle.
// ast-grep-ignore: no-direct-pds-read
const TYPEAHEAD = 'https://public.api.bsky.app/xrpc/app.bsky.actor.searchActorsTypeahead';

/** Where Bluesky serves avatars from. Only these load: the CSP's img-src lists this origin and no other. */
export const AVATAR_ORIGIN = 'https://cdn.bsky.app';

const LIMIT = 5;

const TTL_MS = 10 * 60_000;

const MAX_CACHED = 500;

const typeaheadResponse = z.object({
	actors: z.array(
		z.object({
			did: z.string(),
			handle: z.string(),
			displayName: z.string().optional(),
			avatar: z.string().optional()
		})
	)
});

/**
 * The sign-in page's account typeahead. HappyView only keeps the display names of the accounts this site lists, so
 * prefix searches over everyone go to Bluesky's AppView, from the server: visitors' keystrokes and addresses stay
 * here, the CSP's connect-src stays 'self', and a short in-memory cache answers repeats without asking again.
 * With ACTOR_SEARCH=none it finds nothing.
 */
export class ActorSearch {
	private readonly cache = new Map<string, { at: number; actors: ActorSuggestion[] }>();

	constructor(
		private readonly deps: { logger: Logger; provider: Provider; fetch?: typeof fetch; now?: () => number }
	) {}

	async search(query: string): Promise<ActorSuggestion[]> {
		const term = query.trim().replace(/^@/, '').toLowerCase().slice(0, 100);

		if (term.length < 2 || this.deps.provider === 'none') return [];
		const now = (this.deps.now ?? Date.now)();
		const cached = this.cache.get(term);

		if (cached && now - cached.at < TTL_MS) return cached.actors;
		let actors: ActorSuggestion[];

		try {
			actors = await this.bluesky(term);
		} catch (error) {
			this.deps.logger.warn({ err: error, term }, 'actor typeahead failed');

			return [];
		}

		// A Map keeps insertion order, so the first key is the oldest entry.
		this.cache.delete(term);
		const [oldest] = this.cache.keys();

		if (oldest !== undefined && this.cache.size >= MAX_CACHED) this.cache.delete(oldest);
		this.cache.set(term, { at: now, actors });

		return actors;
	}

	private async bluesky(term: string): Promise<ActorSuggestion[]> {
		const url = new URL(TYPEAHEAD);
		url.searchParams.set('q', term);
		url.searchParams.set('limit', String(LIMIT));
		const response = await (this.deps.fetch ?? fetch)(url, { signal: AbortSignal.timeout(3_000) });

		if (!response.ok) throw new Error(`searchActorsTypeahead ${response.status}`);

		return typeaheadResponse
			.parse(await response.json())
			.actors.slice(0, LIMIT)
			.map(({ did, handle, displayName, avatar }) => ({
				did,
				handle,
				displayName: displayName?.trim() || undefined,
				avatar: avatar?.startsWith(`${AVATAR_ORIGIN}/`) ? avatar : undefined
			}));
	}
}
