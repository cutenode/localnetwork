import { requireManager } from '#lib/server/manage.ts';
import type { LayoutServerLoad } from './$types';

// Pages below await this (`parent()`), or check again themselves, before they read anything, so nothing loads for
// someone who doesn't manage it.
export const load: LayoutServerLoad = async ({ locals, url, params }) => {
	// With counts, so the press kits and shows its pages stream in hold their room while they load.
	const { project } = await requireManager(locals, url, params.slug, { counts: true });

	return { project };
};
