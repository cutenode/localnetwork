import { pageMeta } from '#lib/server/meta.ts';
import { toolCards } from '#lib/tools/index.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals }) => ({
	tools: toolCards(),
	meta: pageMeta(locals.app.config.publicUrl, {
		title: 'Tools',
		description:
			'Tools for DJs from Local Network. Each one stands on its own and signs in with your Atmosphere account.',
		path: '/tools'
	})
});
