import { loadProfilePage } from '#lib/server/profile-page.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = ({ params, locals, url }) => loadProfilePage(locals, params.handle, url.search);
