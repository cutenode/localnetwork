import { requireManager } from '#lib/server/manage.ts';
import type { LayoutServerLoad } from './$types';

// Pages below await this (`parent()`), or check again themselves, before they read anything, so nothing loads for
// someone who doesn't manage it.
export const load: LayoutServerLoad = async ({ locals, url, params }) => {
	const { project } = await requireManager(locals, url, params.slug);

	return { project };
};
