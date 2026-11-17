import { json } from '@sveltejs/kit';
import { z } from 'zod';
import type { RequestHandler } from './$types';

const notifySpaceUpdateInput = z.object({
	space: z.string(),
	token: z.string().max(512),
	update: z.object({ $type: z.string() })
});

/** Updates that need a fresh credential: the authority renews a lapsed registration with it, and confirms a deletion. */
const REAUTHORIZE = new Set([
	'com.atproto.space.notifySpaceUpdate#spaceDeleted',
	'com.atproto.space.notifySpaceUpdate#registrationExpired'
]);

/**
 * A space's updates, sent by its host to the endpoint the syncer registered with a token only the two of them hold.
 * Each one starts a sync, which reads what changed from the host rather than trusting the notice.
 */
export const POST: RequestHandler = async ({ request, locals }) => {
	const { app, log } = locals;
	const input = notifySpaceUpdateInput.safeParse(await request.json().catch(() => null));

	if (!input.success) return json({ error: 'InvalidRequest' }, { status: 400 });
	const { space, token, update } = input.data;

	if (!(await app.syncer.isRegistered(space, token))) {
		log.warn('rejected notifySpaceUpdate', { space });

		return json({ error: 'AuthenticationRequired' }, { status: 401 });
	}

	const sync = REAUTHORIZE.has(update.$type) ? app.syncer.reauthorize(space) : app.syncer.sync(space);
	void sync.catch((error) => log.warn('notified sync failed', { err: error, space }));

	return json({});
};
