import { XRPCError } from '@atproto/api';
import type { Viewer } from '#lib/types.ts';
import type { RepoClient } from '../atproto/repo-client';
import { type Core, ReauthError, UserError } from '../core';

/**
 * Who writes `actor`'s records into a shared space: their own session, or the root on their behalf (the record then
 * names them in `onBehalfOf`) when their PDS can't write to spaces or their session lapsed.
 */
export async function spaceWriter(core: Core, actor: Viewer): Promise<{ client: RepoClient; onBehalf: boolean }> {
	if (actor.spacesCapable) {
		const client = await core.userClient(actor.did);

		if (client) return { client, onBehalf: false };
	}

	return { client: await core.root(), onBehalf: true };
}

/**
 * Runs `write` with `did`'s own repo client. These records (follows, shows, calendar events) live in the account's
 * public repo, so nobody can write them on its behalf; `message` says what signing in again lets them do.
 */
export async function writeOwn<T>(
	core: Core,
	did: string,
	message: string,
	write: (client: RepoClient) => Promise<T>
): Promise<T> {
	const client = await core.userClient(did);

	if (!client) throw new ReauthError(message);

	try {
		return await write(client);
	} catch (error) {
		// How a PDS answers a write the session's scopes don't cover: 403 with "Missing required scope".
		const missingScope =
			error instanceof XRPCError && [401, 403].includes(Number(error.status)) && /scope/i.test(error.message);

		if (!missingScope) throw error;
		core.logger.info({ did, err: error }, 'a write needs a scope the session lacks');

		// A sign-in since the app asked for these writes that still didn't get them means the PDS won't grant them, so
		// signing in again would only come back here.
		if ((await core.accounts.get(did))?.publicWrites === 0)
			throw new UserError("Your account's server doesn't allow this yet");
		throw new ReauthError(message);
	}
}
