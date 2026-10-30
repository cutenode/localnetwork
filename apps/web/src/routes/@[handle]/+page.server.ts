import { clientIp } from '#lib/server/http.ts';
import { loadProfilePage } from '#lib/server/profile-page.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = (event) =>
	loadProfilePage(event.locals, event.params.handle, event.url.search, clientIp(event));
