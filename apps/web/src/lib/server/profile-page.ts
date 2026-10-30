import { isValidDid, isValidHandle } from '@atproto/syntax';
import { error, isRedirect, redirect } from '@sveltejs/kit';
import { accountLabel, profileHref } from '#lib/format.ts';
import type { AccountView } from '#lib/types.ts';
import type { IdentityResolver } from './atproto/identity';
import { unlessOffline } from './happyview/offline';

const NOT_FOUND = 'No account goes by that name';

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
 * What a person's public profile page loads: their profile, the DJ stats and sets they publish (streamed, unless
 * they're fresh in the cache), and the active projects they manage, streamed too. Every account has a profile page:
 * one without a profile record shows its Bluesky display name when the directory keeps it, else its handle, and never
 * asks its PDS. Each address that names no account counts against `ip`, since resolving it may have meant asking a
 * host the visitor chose; past the limit, the page answers 429 without resolving anything.
 */
export async function loadProfilePage(locals: App.Locals, param: string, search: string, ip: string) {
	const { app } = locals;
	const misses = app.limits.profileMisses;

	// Charged before resolving and given back when the address names an account, so requests made together can't all
	// get through before any is charged.
	if (!misses.take(ip)) error(429, 'Too many lookups. Try again in a few minutes.');
	let account: AccountView;

	try {
		account = await canonicalAccount(app.identity, param, search, () => error(404, NOT_FOUND));
	} catch (cause) {
		if (isRedirect(cause)) misses.refund(ip);
		throw cause;
	}

	misses.refund(ip);
	const profile = await app.profiles.get(account.did);
	const displayName = profile ? null : await app.profiles.blueskyName(account.did);
	const linked = (profile?.record.performer?.mixes ?? []).flatMap((mix) => mix.set ?? []);

	return {
		profile,
		name: profile?.record.basics.name ?? displayName ?? accountLabel(account),
		account,
		canonical: `${app.config.publicUrl}${profileHref(account)}`,
		dj: app.dj.fresh([account]) ?? app.dj.activity([account]),
		sets: linked.length ? app.sets.linked(linked) : undefined,
		playsIn: unlessOffline(locals.log, async () =>
			(await app.projects.managedBy(account.did)).filter((project) => project.status === 'active')
		)
	};
}
