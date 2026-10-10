import { isRequestKind } from '#lib/server/services/requests.ts';
import type { RequestStatus } from '#lib/types.ts';
import type { PageServerLoad } from './$types';

const STATUSES = [
	'pending',
	'changes_requested',
	'approved',
	'rejected',
	'withdrawn'
] as const satisfies readonly RequestStatus[];

export const load: PageServerLoad = async ({ locals, url }) => {
	const statusParam = url.searchParams.get('status') ?? 'pending';

	const status = statusParam === 'all' ? undefined : (STATUSES.find((value) => value === statusParam) ?? 'pending');

	const kindParam = url.searchParams.get('kind');
	const kind = isRequestKind(kindParam) ? kindParam : undefined;

	const [requests, counts] = await Promise.all([
		locals.app.requests.list({ status, kind, limit: 200 }),
		locals.app.requests.counts()
	]);

	return { requests, counts, status: status ?? 'all', kind: kind ?? '' };
};
