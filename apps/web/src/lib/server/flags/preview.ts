import { createHmac, timingSafeEqual } from 'node:crypto';
import { z } from 'zod';
import type { FlagPreview } from '#lib/types.ts';
import { FLAGS } from './catalog';

/** A staff member's own view of some flags, on or off, in one browser. */
export const PREVIEW_COOKIE = 'ln_flag_preview';

/** How long a preview lasts: long enough to try a feature, short enough that a forgotten one doesn't linger. */
export const PREVIEW_MS = 3_600_000;

const sealed = z.object({ did: z.string(), flags: z.record(z.string(), z.boolean()), expiresAt: z.number() });

const sign = (payload: string, sessionToken: string) =>
	createHmac('sha256', sessionToken).update(payload).digest('base64url');

/**
 * The cookie's value: the preview, signed with the browser's session token. Only that session can have made it, and
 * signing out or in as someone else leaves it unreadable, with no secret of its own to keep.
 */
export function sealPreview(preview: z.infer<typeof sealed>, sessionToken: string): string {
	const payload = Buffer.from(JSON.stringify(preview)).toString('base64url');

	return `${payload}.${sign(payload, sessionToken)}`;
}

/** The flags a cookie previews for `did` in this session, or null when it's missing, someone else's or expired. */
export function openPreview(
	value: string | undefined,
	sessionToken: string | undefined,
	did: string,
	now = Date.now()
): FlagPreview | null {
	const [payload, signature, ...rest] = value?.split('.') ?? [];

	if (!payload || !signature || rest.length > 0 || !sessionToken) return null;
	const expected = Buffer.from(sign(payload, sessionToken));
	const given = Buffer.from(signature);

	if (given.length !== expected.length || !timingSafeEqual(given, expected)) return null;
	const preview = sealed.safeParse(JSON.parse(Buffer.from(payload, 'base64url').toString()));

	if (!preview.success || preview.data.did !== did || preview.data.expiresAt <= now) return null;

	return Object.fromEntries(
		FLAGS.filter(({ key }) => key in preview.data.flags).map(({ key }) => [key, preview.data.flags[key]])
	);
}
