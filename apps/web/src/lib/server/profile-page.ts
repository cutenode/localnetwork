import { isValidDid, isValidHandle } from '@atproto/syntax';
import { error, redirect } from '@sveltejs/kit';
import { profileHref } from '#lib/format.ts';
import type { AccountView } from '#lib/types.ts';
import type { IdentityResolver } from './atproto/identity';
import { unlessOffline } from './happyview/offline';

const NOT_FOUND = 'No profile here';

/**
 * The account a profile URL names (`/@<handle>` or `/@<did>`), redirecting to its canonical one: a DID with a
 * verified handle goes to the handle, and a handle goes to the one its DID document names, which also lowercases it.
 * A handle counts only when its DID document names it back (see IdentityResolver.resolve), so anything else is a 404.
 */
async function canonicalAccount(
	identity: Pick<IdentityResolver, 'resolve' | 'resolveHandle'>,
	param: string,
	search: string,
	miss: () => never
): Promise<AccountView> {
	if (param.startsWith('did:')) {
		const resolved = isValidDid(param) ? await identity.resolve(param).catch(() => null) : null;

		if (!resolved) miss();

		if (resolved.handle) redirect(302, `${profileHref(resolved)}${search}`);

		return { did: param, handle: null };
	}

	const handle = param.toLowerCase();
	const did = isValidHandle(handle) ? await identity.resolveHandle(handle) : null;
	const resolved = did ? await identity.resolve(did).catch(() => null) : null;

	if (!resolved?.handle) miss();

	if (resolved.handle !== param) redirect(302, `${profileHref(resolved)}${search}`);

	return { did: resolved.did, handle: resolved.handle };
}

/**
 * What a person's public profile page loads: their profile (404 without one), the DJ stats and sets they publish
 * (streamed, unless they're fresh in the cache), and the active projects they manage, streamed too. Each address that
 * names no account counts against `ip`, since resolving it may have meant asking a host the visitor chose; past the
 * limit, the page answers 429 without resolving anything.
 */
export async function loadProfilePage(locals: App.Locals, param: string, search: string, ip: string) {
	const { app } = locals;
	const misses = app.limits.profileMisses;

	if (!misses.has(ip)) error(429, 'Too many lookups. Try again in a few minutes.');

	const miss = () => {
		misses.take(ip);
		error(404, NOT_FOUND);
	};

	const account = await canonicalAccount(app.identity, param, search, miss);
	const profile = await app.profiles.get(account.did);

	if (!profile) error(404, NOT_FOUND);
	const linked = (profile.record.performer?.mixes ?? []).flatMap((mix) => mix.set ?? []);

	return {
		profile,
		account,
		canonical: `${app.config.publicUrl}${profileHref(account)}`,
		dj: app.dj.fresh([account]) ?? app.dj.activity([account]),
		sets: linked.length ? app.sets.linked(linked) : undefined,
		playsIn: unlessOffline(locals.log, async () =>
			(await app.projects.managedBy(account.did)).filter((project) => project.status === 'active')
		)
	};
}
