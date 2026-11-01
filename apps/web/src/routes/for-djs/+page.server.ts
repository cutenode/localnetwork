import { pageMeta } from '#lib/server/meta.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals }) => ({
	meta: pageMeta(locals.app.config.publicUrl, {
		title: 'For DJs',
		description: 'List or claim your DJ project so promoters near you can find and book you.',
		path: '/for-djs'
	})
});
