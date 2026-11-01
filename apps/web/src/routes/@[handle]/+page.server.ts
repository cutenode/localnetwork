import { clientIp } from '#lib/server/http.ts';
import { profileMeta } from '#lib/server/meta.ts';
import { profileRecords, withRecords } from '#lib/server/meta-records.ts';
import { loadProfilePage } from '#lib/server/profile-page.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async (event) => {
	const page = await loadProfilePage(event.locals, event.params.handle, event.url.search, clientIp(event));

	return {
		...page,
		meta: await withRecords(profileMeta(event.locals.app.config.publicUrl, page.account, page.profile, page.name), () =>
			profileRecords(page.profile)
		)
	};
};
