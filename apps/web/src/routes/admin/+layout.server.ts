import { requireStaff } from '#lib/server/http.ts';
import type { StaffGrant } from '#lib/types.ts';
import type { LayoutServerLoad } from './$types';

/** Optional so a page rendered on its own, in a test or a story, offers nothing that takes a grant. */
type AdminData = { grants?: StaffGrant[] };

export const load: LayoutServerLoad = async ({ locals, url }): Promise<AdminData> => {
	const viewer = requireStaff(locals, url);

	return { grants: await locals.app.staff.grants(viewer.did) };
};
