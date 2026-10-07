import { isProjectStatus } from '#lib/server/services/projects.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url }) => {
	const q = url.searchParams.get('q')?.slice(0, 100) ?? '';
	const statusParam = url.searchParams.get('status') ?? '';
	const status = isProjectStatus(statusParam) ? statusParam : undefined;
	const unclaimed = url.searchParams.has('unclaimed');

	const result = await locals.app.projects.search({
		q: q || undefined,
		includeInactive: true,
		status,
		unclaimed,
		sort: q ? 'relevance' : 'name',
		pageSize: 60,
		page: Number(url.searchParams.get('page')) || 1
	});

	return {
		items: result.items,
		total: result.total,
		page: result.page,
		pageSize: result.pageSize,
		q,
		status: status ?? '',
		unclaimed
	};
};
