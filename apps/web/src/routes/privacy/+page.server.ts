import type { PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals }) => ({ retentionDays: locals.app.epkViews.retentionDays });
