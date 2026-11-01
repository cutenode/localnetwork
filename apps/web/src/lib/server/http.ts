import { error, fail, isHttpError, isRedirect, redirect } from '@sveltejs/kit';
import type { Viewer } from '#lib/types.ts';
import { RecordValidationError } from './atproto/lexicons';
import { ReauthError, UserError } from './core';

type LocalsLike = { viewer: Viewer | null };

/** The CIDs records are written with: base32 CIDv1, as the lexicon's `cid` format accepts. */
export const CID = /^b[a-z2-7]{20,100}$/;

export function requireViewer(locals: LocalsLike, url: URL): Viewer {
	if (!locals.viewer) redirect(303, `/login?next=${encodeURIComponent(url.pathname + url.search)}`);

	return locals.viewer;
}

export function requireStaff(locals: LocalsLike, url: URL): Viewer {
	const viewer = requireViewer(locals, url);

	if (!viewer.isStaff) error(403, 'Only moderators and admins can see this page');

	return viewer;
}

export function requireAdmin(locals: LocalsLike, url: URL): Viewer {
	const viewer = requireViewer(locals, url);

	if (!viewer.isAdmin) error(403, 'Only admins can see this page');

	return viewer;
}

const isUnder = (routeId: string | null, area: string) => routeId === area || !!routeId?.startsWith(`${area}/`);

/**
 * Guards the signed-in areas from the server hook: the dashboard needs a viewer and the staff
 * console needs staff. A layout's check alone isn't enough, since SvelteKit runs a page's loads
 * alongside its layouts' and still sends their data on a data request when the layout refuses. It
 * goes by the route SvelteKit matched (`event.route.id`), not the path: routes match the decoded
 * path, so `/%61dmin` reaches `/admin` while its path doesn't start with it.
 */
export function guardArea(locals: LocalsLike, url: URL, routeId: string | null): void {
	if (isUnder(routeId, '/dashboard')) requireViewer(locals, url);

	if (isUnder(routeId, '/admin')) requireStaff(locals, url);
}

const SAME_SITE = 'http://same.site';

/**
 * Only allow same-site relative redirects after sign-in. Browsers drop tabs and newlines from a
 * URL and read a backslash as a slash, so those are refused outright, and what's left has to
 * resolve to this site the way a browser would resolve it.
 */
export function safeNext(value: string | null | undefined, fallback = '/dashboard'): string {
	if (!value || !value.startsWith('/') || /[\s\\\p{Cc}]/u.test(value)) return fallback;
	const url = new URL(value, SAME_SITE);

	// The path as resolved, since a caller that builds a URL on it gets that: `/.//evil.test` resolves to the path
	// `//evil.test`, which on its own is another site.
	if (url.origin !== SAME_SITE || url.pathname.startsWith('//') || url.pathname.includes('\\')) return fallback;

	return url.pathname + url.search + url.hash;
}

/** A same-site path with `params` added to its query, checked again once built, or `fallback` for any other. */
export function withQuery(path: string, params: Record<string, string>, fallback = '/dashboard'): string {
	const safe = safeNext(path, '');

	if (!safe) return fallback;
	const url = new URL(safe, SAME_SITE);

	for (const [name, value] of Object.entries(params)) url.searchParams.set(name, value);

	return safeNext(url.pathname + url.search + url.hash, fallback);
}

/**
 * Runs a form action body, turning user-facing errors into `fail()` results
 * so the page re-renders with a message instead of an error page.
 */
export async function act<T, V extends object = never>(run: () => Promise<T>, values?: V) {
	try {
		return await run();
	} catch (cause) {
		if (isRedirect(cause) || isHttpError(cause)) throw cause;

		if (cause instanceof UserError)
			return fail(cause.status, { error: cause.message, values, reauth: cause instanceof ReauthError });

		// The same shape as above: TypeScript would drop `reauth` from the action's result type otherwise.
		if (cause instanceof RecordValidationError)
			return fail(400, { error: `That doesn't fit the record format: ${cause.message}`, values, reauth: false });
		throw cause;
	}
}

export function text(form: FormData, name: string, max = 10_000): string {
	const value = form.get(name);

	return value === null || value instanceof File ? '' : value.trim().slice(0, max);
}

export function optionalText(form: FormData, name: string, max = 10_000): string | undefined {
	return text(form, name, max) || undefined;
}

export function optionalInt(form: FormData, name: string, min: number, max: number): number | undefined {
	return wholeNumber(text(form, name, 32), name, min, max);
}

/** Parses an optional whole number from a raw (trimmed) form value. */
export function wholeNumber(raw: string, name: string, min: number, max: number): number | undefined {
	if (!raw) return undefined;
	const value = Number(raw);

	if (!Number.isInteger(value) || value < min || value > max)
		throw new UserError(`${name} must be a whole number from ${min} to ${max}`);

	return value;
}

/** An optional three-letter currency code, uppercased. */
export function optionalCurrency(form: FormData, name: string): string | undefined {
	return currencyCode(text(form, name, 32));
}

/** An optional three-letter currency code from a raw (trimmed) form value, uppercased. */
export function currencyCode(raw: string): string | undefined {
	const value = raw.toUpperCase();

	if (value && !/^[A-Z]{3}$/.test(value))
		throw new UserError('Use a three-letter currency code (like USD, EUR, or GBP)');

	return value || undefined;
}

const graphemes = (value: string) => [...new Intl.Segmenter('en', { granularity: 'grapheme' }).segment(value)].length;

/** Checks a lexicon's grapheme cap, with a friendly error. */
export function limit(field: string, value: string | undefined, max: number): string | undefined {
	if (value === undefined) return undefined;

	if (graphemes(value) > max) throw new UserError(`${field} can be at most ${max} characters`);

	return value;
}

/** A form's text values, echoed back into it on errors. Repeated fields keep their last value. */
export function stringValues(form: FormData): Record<string, string> {
	return Object.fromEntries(
		[...form.entries()].filter((entry): entry is [string, string] => !(entry[1] instanceof File))
	);
}

export function all(form: FormData, name: string, max = 50): string[] {
	return form
		.getAll(name)
		.filter((value): value is string => !(value instanceof File))
		.map((value) => value.trim())
		.filter(Boolean)
		.slice(0, max);
}

export function clientIp(event: { getClientAddress(): string }): string {
	try {
		return event.getClientAddress();
	} catch {
		return 'unknown';
	}
}

/**
 * Lets a page that shows the same thing to everyone be cached for `seconds`: by anyone when the request has no
 * session cookie, by the browser alone when it has one. A signed-in page carries the account, and one whose session
 * ended asks to sign in again or clears the cookie, so neither may reach a shared cache. It varies on the cookie, so
 * signing in or out isn't answered from a copy made before.
 */
export function cacheBriefly(
	event: { locals: LocalsLike & { sessionToken?: string }; setHeaders(headers: Record<string, string>): void },
	seconds = 60
): void {
	const isAnonymous = !event.locals.viewer && !event.locals.sessionToken;

	event.setHeaders({ 'cache-control': `${isAnonymous ? 'public' : 'private'}, max-age=${seconds}`, vary: 'Cookie' });
}
