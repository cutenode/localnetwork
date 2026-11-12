import { type Cookies, error } from '@sveltejs/kit';
import type { StaffGrant, Viewer } from '#lib/types.ts';
import { IdentityError } from '../atproto/identity';
import { UserError } from '../core';
import { PREVIEW_COOKIE, PREVIEW_MS, sealPreview } from '../flags/preview';
import { all, requireStaff, text, wholeNumber } from '../http';
import type { FlagGroupInput, FlagInput } from '../services/flags';

type Resolver = { resolveIdentifier(identifier: string): Promise<string> };

type StaffLocals = { viewer: Viewer | null; app: { staff: { grants(did: string): Promise<StaffGrant[]> } } };

/** Staff holding `grant`. Admins hold them all, and a moderator only what an admin gave them. */
export async function requireGrant(locals: StaffLocals, url: URL, grant: StaffGrant): Promise<Viewer> {
	const viewer = requireStaff(locals, url);

	if (!(await locals.app.staff.grants(viewer.did)).includes(grant))
		error(403, 'An admin can give you access to this page');

	return viewer;
}

type PreviewLocals = { sessionToken?: string; app: { config: { publicUrl: string } } };

/** Saves what `viewer` previews in this browser, for the next hour, or drops the cookie once nothing's left in it. */
export function writePreview(cookies: Cookies, locals: PreviewLocals, viewer: Viewer, flags: Record<string, boolean>) {
	if (Object.keys(flags).length === 0 || !locals.sessionToken) {
		cookies.delete(PREVIEW_COOKIE, { path: '/' });

		return;
	}

	cookies.set(
		PREVIEW_COOKIE,
		sealPreview({ did: viewer.did, flags, expiresAt: Date.now() + PREVIEW_MS }, locals.sessionToken),
		{
			path: '/',
			httpOnly: true,
			sameSite: 'lax',
			secure: locals.app.config.publicUrl.startsWith('https:'),
			maxAge: PREVIEW_MS / 1000
		}
	);
}

/** The accounts in a field holding a handle or DID on each line, as DIDs, each once. */
async function accounts(form: FormData, name: string, identity: Resolver): Promise<string[]> {
	const lines = new Set(
		text(form, name, 400_000)
			.split(/[\s,]+/)
			.filter(Boolean)
	);

	const dids = await Promise.all(
		[...lines].map((line) =>
			identity.resolveIdentifier(line.replace(/^@/, '')).catch((cause: unknown) => {
				if (cause instanceof IdentityError) throw new UserError(cause.message);
				throw cause;
			})
		)
	);

	return [...new Set(dids)];
}

export async function flagInput(form: FormData, identity: Resolver): Promise<FlagInput> {
	return {
		enabled: form.get('off') !== 'on',
		everyone: form.get('everyone') === 'on',
		percentage: wholeNumber(text(form, 'percentage', 8), 'Percentage', 0, 100) ?? 0,
		groups: all(form, 'groups', 100),
		dids: await accounts(form, 'accounts', identity)
	};
}

export async function flagGroupInput(form: FormData, identity: Resolver): Promise<FlagGroupInput> {
	const name = text(form, 'name', 64);

	if (!name) throw new UserError('Give the group a name');

	return {
		name,
		description: text(form, 'description', 300) || undefined,
		members: await accounts(form, 'members', identity)
	};
}
