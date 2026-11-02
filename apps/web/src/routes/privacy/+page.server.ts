import { pageMeta } from '#lib/server/meta.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals }) => ({
	retentionDays: locals.app.epkViews.retentionDays,
	meta: pageMeta(locals.app.config.publicUrl, {
		title: 'Privacy',
		description: 'What Local Network stores, where each kind of data lives, and who can read it.',
		path: '/privacy'
	})
});
