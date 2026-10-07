import { requireStaff } from '#lib/server/http.ts';
import type { LayoutServerLoad } from './$types';

export const load: LayoutServerLoad = ({ locals, url }) => {
	requireStaff(locals, url);

	return {};
};
