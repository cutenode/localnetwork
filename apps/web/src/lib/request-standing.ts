import type { RequestView } from './types';

/** A day the way a sentence says it, like "3 Nov", with the year once it isn't this one. */
export function dayLabel(iso: string, now = Date.now()): string {
	const date = new Date(iso);
	const isThisYear = date.getFullYear() === new Date(now).getFullYear();

	return new Intl.DateTimeFormat('en-GB', {
		day: 'numeric',
		month: 'short',
		year: isThisYear ? undefined : 'numeric'
	}).format(date);
}

/** Who has a request and since when, the same words wherever one shows. */
export function requestStanding(
	request: Pick<RequestView, 'status' | 'updatedAt' | 'decision'>,
	now = Date.now()
): string {
	const day = dayLabel(request.decision?.at ?? request.updatedAt, now);

	if (request.status === 'pending')
		return `With staff since ${dayLabel(request.updatedAt, now)}. Nothing needed from you.`;

	if (request.status === 'changes_requested') return `Staff asked for changes on ${day}.`;

	if (request.status === 'approved') return `Approved on ${day}.`;

	return request.status === 'rejected' ? `Turned down on ${day}.` : `Withdrawn on ${day}.`;
}
