import { randomUUID } from 'node:crypto';
import type { Cookies } from '@sveltejs/kit';

/** A random id for a signed-out browser, so a flag on for a percentage gives it the same answer on every request. */
export const VISITOR_COOKIE = 'ln_flag_visitor';

/** What `randomUUID` makes. A cookie holding anything else is replaced with a new one. */
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

/** In front of the cookie's UUID in the id a flag buckets. A DID starts `did:`, so a visitor's id is never an account's. */
const PREFIX = 'visitor:';

/** How long the browser keeps its id: longer than a release flag's rollout runs. */
const MAX_AGE_S = 365 * 86_400;

/**
 * The id a signed-out visitor is bucketed by, from their cookie, after giving them one if they have none, and whether
 * it's new. It's only for rollouts: the server doesn't store it or log it. A visitor who clears it can land on the
 * other side of a rollout, which is fine since flags never keep anyone out.
 */
export function flagVisitor(cookies: Pick<Cookies, 'get' | 'set'>, secure: boolean) {
	const saved = cookies.get(VISITOR_COOKIE);

	if (saved && UUID.test(saved)) return { id: PREFIX + saved, isNew: false };
	const uuid = randomUUID();

	cookies.set(VISITOR_COOKIE, uuid, { path: '/', httpOnly: true, sameSite: 'lax', secure, maxAge: MAX_AGE_S });

	return { id: PREFIX + uuid, isNew: true };
}
