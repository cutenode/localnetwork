import { performerUri, parsePerformerUri } from '#lib/server/performer-uri.ts';
import type { BLOCK_KINDS } from '#lib/taxonomy/options.ts';
import type { App } from './app';
import { parseSpaceUri, sharedSpace } from './atproto/spaces';
import { UserError } from './core';

export type BlockKind = (typeof BLOCK_KINDS)[number]['value'];

/** A block as the settings page lists it: who, and what kind of thing they are. */
export type BlockView = { rkey: string; kind: BlockKind; name: string };

type Lookups = Pick<App, 'core' | 'identity' | 'performerRegistry' | 'sharedSpaces' | 'accounts' | 'parties'>;

/** The key in an act's or party's page link (`/act/<key>/<name>`), or in a party's space URI. */
function pageKey(kind: 'act' | 'party', input: string): string {
	return parseSpaceUri(input)?.skey ?? input.match(new RegExp(`(?:^|/)${kind}/([^/?#]+)`))?.[1] ?? input;
}

/**
 * The URI a block names for `input`: a handle (or DID) for a person, and for an act or party its page link, its key, or
 * a party's space URI.
 */
export async function blockSubject(app: Lookups, kind: BlockKind, input: string): Promise<string> {
	const wanted = input.trim().replace(/^@/, '');
	const { rootDid } = app.core;

	if (kind === 'profile') {
		const did = await app.identity.resolveIdentifier(wanted).catch(() => null);

		if (!did) throw new UserError(`There's no account called ${wanted}`);

		return performerUri(rootDid, { kind: 'dj', did });
	}

	const skey = pageKey(kind, wanted);

	const found =
		kind === 'act'
			? (await app.performerRegistry.byKey(skey))?.kind === 'act'
			: !!(await app.sharedSpaces.get(sharedSpace(rootDid, 'party', skey)));

	if (!found) throw new UserError(`There's no ${kind} at that link`);

	// An act's and a party's listings share the root's listing collection, keyed by their space's key.
	return performerUri(rootDid, { kind: 'act', skey });
}

/** Each block with the name it shows under: a person's handle, an act's name, or a party's name. */
export async function blockViews(app: Lookups, blocks: { rkey: string; subject: string }[]): Promise<BlockView[]> {
	return Promise.all(
		blocks.map(async ({ rkey, subject }): Promise<BlockView> => {
			const ref = parsePerformerUri(app.core.rootDid, subject);

			if (ref?.kind === 'dj') {
				const { handle } = await app.accounts.view(ref.did);

				return { rkey, kind: 'profile', name: handle ? `@${handle}` : ref.did };
			}

			if (ref?.kind === 'act') {
				const act = await app.performerRegistry.byKey(ref.skey);

				if (act?.kind === 'act') return { rkey, kind: 'act', name: act.act.basics.name };

				const party = await app.parties.bySkey(ref.skey);

				if (party) return { rkey, kind: 'party', name: party.basics.name };
			}

			// What it named is gone; the block stays until someone takes it away.
			return { rkey, kind: 'profile', name: 'Someone no longer here' };
		})
	);
}
