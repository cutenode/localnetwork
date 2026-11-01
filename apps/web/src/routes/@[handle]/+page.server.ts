import { clientIp } from '#lib/server/http.ts';
import { listed, profileMeta } from '#lib/server/meta.ts';
import { profileRecords } from '#lib/server/meta-records.ts';
import { loadProfilePage } from '#lib/server/profile-page.ts';
import { personProfile } from '#lib/server/structured-data.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async (event) => {
	const page = await loadProfilePage(event.locals, event.params.handle, event.url.search, clientIp(event));

	return {
		...page,
		meta: await listed(profileMeta(event.locals.app.config.publicUrl, page.account, page.profile, page.name), {
			records: () => profileRecords(page.profile),
			jsonLd: page.profile && personProfile(page.profile, page.name)
		})
	};
};
