import { soon } from '#lib/server/home.ts';
import { requireViewer } from '#lib/server/http.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url }) => {
	const viewer = requireViewer(locals, url);
	const { app, log } = locals;

	// Each row says where things stand from its own source, and a source that fails costs only that row's line.
	const quietly = <T>(work: Promise<T>) =>
		soon(
			work.catch((error) => {
				log.warn('could not read a line for the account page', { err: error });

				return null;
			})
		);

	const [channels, latest, requests, held] = await Promise.all([
		quietly(
			Promise.all([app.notificationChannels.view(viewer.did, 'email'), app.notificationChannels.view(viewer.did, 'dm')])
		),
		quietly(app.audit.list({ actor: viewer.did, limit: 1 })),
		quietly(app.requests.list({ requester: viewer.did, limit: 50 })),
		quietly(app.heldRecords.list(viewer))
	]);

	return { channels: channels.value, latest: latest.value, requests: requests.value, held: held.value };
};
