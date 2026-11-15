import { unlessOffline } from '#lib/server/happyview/offline.ts';
import { pageMeta } from '#lib/server/meta.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals }) => {
	const { projects, config } = locals.app;

	return {
		// Streamed, so the two doors paint before the directory answers. Null when it's offline.
		stats: unlessOffline(locals.log, async () => {
			const { active, cityCount, unclaimed } = await projects.stats();

			return { djs: active, cities: cityCount, unclaimed };
		}),
		meta: pageMeta(config.publicUrl, {
			title: 'Who Local Network is for',
			description: 'DJs get found and booked. Promoters find DJs who fit the night. Pick your side.',
			path: '/for'
		})
	};
};
