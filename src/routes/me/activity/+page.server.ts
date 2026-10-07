import { requireViewer } from '#lib/server/http.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url }) => {
	const viewer = requireViewer(locals, url);
	const before = Number(url.searchParams.get('before')) || undefined;
	const entries = await locals.app.audit.list({ actor: viewer.did, before, limit: 50 });

	return { entries };
};
