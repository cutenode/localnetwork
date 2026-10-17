import { error, fail, isHttpError, isRedirect, redirect } from '@sveltejs/kit';
import type { Viewer } from '#lib/types.ts';
import { RecordValidationError } from './atproto/lexicons';
import { UserError } from './core';

type LocalsLike = { viewer: Viewer | null };

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

/** Only allow same-site relative redirects after sign-in. */
export function safeNext(value: string | null | undefined, fallback = '/me'): string {
	if (!value || !value.startsWith('/') || value.startsWith('//') || value.startsWith('/\\')) return fallback;

	return value;
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
			return fail(cause.status === 401 ? 401 : cause.status, { error: cause.message, values });

		if (cause instanceof RecordValidationError)
			return fail(400, { error: `That doesn't fit the record format: ${cause.message}`, values });
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
	const value = text(form, name, 32).toUpperCase();

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
 * Lets a page that shows the same thing to everyone be cached for `seconds`: by anyone when nobody is signed in, by
 * the browser alone when someone is, since the page carries their account.
 */
export function cacheBriefly(
	event: { locals: LocalsLike; setHeaders(headers: Record<string, string>): void },
	seconds = 60
): void {
	event.setHeaders({ 'cache-control': `${event.locals.viewer ? 'private' : 'public'}, max-age=${seconds}` });
}
