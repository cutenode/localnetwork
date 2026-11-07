import { soon } from '#lib/server/home.ts';
import { requireViewer } from '#lib/server/http.ts';
import type { PageServerLoad } from './$types';

const PER_PAGE = 50;

export const load: PageServerLoad = async ({ locals, url }) => {
	const viewer = requireViewer(locals, url);
	const before = Number(url.searchParams.get('before')) || undefined;

	// One past the page, so Older only shows when there's something to go to.
	const entries = locals.app.audit.list({ actor: viewer.did, before, limit: PER_PAGE + 1 }).then((found) => ({
		entries: found.slice(0, PER_PAGE),
		older: found.length > PER_PAGE ? found[PER_PAGE - 1].seq : null
	}));

	entries.catch((error) => locals.log.warn('could not read someone their activity', { err: error }));

	return { activity: (await soon(entries)).value };
};
