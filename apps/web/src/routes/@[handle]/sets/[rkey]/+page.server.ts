import { error } from '@sveltejs/kit';
import { profileHref } from '#lib/format.ts';
import { unlessOffline } from '#lib/server/happyview/offline.ts';
import { clientIp } from '#lib/server/http.ts';
import { listed, setMeta } from '#lib/server/meta.ts';
import { setRecords } from '#lib/server/meta-records.ts';
import { pageAccount, personName } from '#lib/server/profile-page.ts';
import { setPlaylist } from '#lib/server/structured-data.ts';
import type { PageServerLoad } from './$types';

/**
 * One of a DJ's sets, under their address. The event it names and the mixes on their active projects' pages that
 * link it are streamed, since both are separate lookups.
 */
export const load: PageServerLoad = async (event) => {
	const { params, locals } = event;
	const { app, log } = locals;
	const account = await pageAccount(locals, params.handle, `/sets/${params.rkey}${event.url.search}`, clientIp(event));
	const set = await app.recordings.set(account.did, params.rkey);

	if (!set) error(404, 'No set here');
	const { profile, name } = await personName(app, account);
	const avatarUrl = profile?.avatarUrl ?? null;

	return {
		account,
		name,
		avatarUrl,
		set,
		event: set.event ? app.shows.eventName(set.event) : null,
		mixes: unlessOffline(log, async () =>
			(await app.projects.managedBy(account.did))
				.filter((project) => project.status === 'active')
				.flatMap(({ slug, profile: { name: project, mixes = [] } }) =>
					mixes.flatMap((mix, index) =>
						mix.set?.uri === set.uri
							? [
									{
										title: mix.title,
										url: mix.url,
										project: { slug, name: project },
										report: mix.tracklist ? index + 1 : null
									}
								]
							: []
					)
				)
		),
		meta: await listed(locals, setMeta(app.config.publicUrl, account, { name, avatarUrl }, set), {
			records: () => setRecords(set.uri, account.did),
			jsonLd: setPlaylist(set, { name, path: profileHref(account) })
		})
	};
};
