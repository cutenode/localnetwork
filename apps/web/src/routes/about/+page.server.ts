import { pageMeta } from '#lib/server/meta.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals }) => ({
	meta: pageMeta(locals.app.config.publicUrl, {
		title: 'How it works',
		description: 'How Local Network keeps projects, requests, bookings and its audit log on the AT Protocol.',
		path: '/about'
	})
});
