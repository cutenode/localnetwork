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
 * `rest` is what the URL has after the account (a path under it, then the query), which the redirect keeps.
 */
async function canonicalAccount(
	identity: Pick<IdentityResolver, 'resolve' | 'resolveHandle'>,
	param: string,
	rest: string,
	miss: () => never
): Promise<AccountView> {
	if (param.startsWith('did:')) {
		const resolved = isValidDid(param) ? await identity.resolve(param).catch(() => null) : null;

		if (!resolved) miss();

		if (resolved.handle) redirect(302, `${profileHref(resolved)}${rest}`);

		return { did: param, handle: null };
	}

	const handle = param.toLowerCase();
	const did = isValidHandle(handle) ? await identity.resolveHandle(handle) : null;
	const resolved = did ? await identity.resolve(did).catch(() => null) : null;

	if (!resolved?.handle) miss();

	if (resolved.handle !== param) redirect(302, `${profileHref(resolved)}${rest}`);

	return { did: resolved.did, handle: resolved.handle };
}

/**
 * The account a page under `/@<handle>` is about, redirecting to its canonical address (see canonicalAccount). Each
 * address that names no account counts against `ip`, since resolving it may have meant asking a host the visitor
 * chose; past the limit, the page answers 429 without resolving anything.
 */
export async function pageAccount(locals: App.Locals, param: string, rest: string, ip: string): Promise<AccountView> {
	const misses = locals.app.limits.profileMisses;

	// Charged before resolving and given back when the address names an account, so requests made together can't all
	// get through before any is charged.
	if (!misses.take(ip)) error(429, 'Too many lookups. Try again in a few minutes.');
	let account: AccountView;

	try {
		account = await canonicalAccount(locals.app.identity, param, rest, () => error(404, NOT_FOUND));
	} catch (cause) {
		if (isRedirect(cause)) misses.refund(ip);
		throw cause;
	}

	misses.refund(ip);

	return account;
}

/**
 * A person's profile, the name their pages go by (the profile's, else their Bluesky display name when the directory
 * keeps it, else their handle), and their pronouns: their Bluesky profile's, else the ones profiles kept before
 * pronouns came from there. Never asks their PDS.
 */
export async function personName(app: App.Locals['app'], account: AccountView) {
	const [profile, bluesky] = await Promise.all([app.profiles.get(account.did), app.profiles.bluesky(account.did)]);

	return {
		profile,
		name: profile?.record.basics.name ?? bluesky.name ?? accountLabel(account),
		pronouns: bluesky.pronouns ?? profile?.record.basics.pronouns ?? null
	};
}

/**
 * What a person's public profile page loads: their profile, the DJ stats and sets they publish (streamed, unless
 * they're fresh in the cache), and the active projects they manage, streamed too. A link that opens one of the page's
 * tabs (`?tab=`) gets both with the page instead, so the tab it names works without scripts. Every account has a
 * profile page, named as personName says even without a profile record.
 */
export async function loadProfilePage(locals: App.Locals, param: string, search: string, ip: string) {
	const { app } = locals;
	const account = await pageAccount(locals, param, search, ip);
	const { profile, name, pronouns } = await personName(app, account);
	const linked = (profile?.record.performer?.mixes ?? []).flatMap((mix) => mix.set ?? []);
	const opensTab = new URLSearchParams(search).has('tab');
	const dj = app.dj.fresh([account]) ?? app.dj.activity([account]);

	const playsIn = unlessOffline(locals.log, async () =>
		(await app.projects.managedBy(account.did)).filter((project) => project.status === 'active')
	);

	return {
		profile,
		name,
		pronouns,
		account,
		dj: opensTab ? await dj : dj,
		sets: linked.length ? app.sets.linked(linked) : undefined,
		playsIn: opensTab ? await playsIn : playsIn
	};
}
