import type { AuditAction } from '#lib/server/audit/log.ts';
import type { AuditEntryRow, CopyStatus } from '#lib/server/db/schema.ts';

/** Summaries start with a past-tense verb, so they carry on after a name. They hold what the action alone can't: which act, which show. */
const said = (summary: string) => summary.charAt(0).toLowerCase() + summary.slice(1);

/** Words that don't depend on the summary, which adds nothing a person needs. */
const words = (fixed: string) => () => fixed;

const withoutRevision = (summary: string) => said(summary).replace(/ (\(revision \d+\)|to revision \d+)$/, '');

/**
 * What each action reads as after the actor's name. Keyed on every action so a new one has to say how it reads, but for
 * the few whose summary already says who did it.
 */
const PHRASES: Record<
	Exclude<AuditAction, 'epk.view' | 'system.bootstrap' | 'system.space'>,
	(summary: string) => string
> = {
	'auth.login': words('signed in'),
	'auth.logout': words('signed out'),
	'project.create': withoutRevision,
	'project.update': withoutRevision,
	'project.status': said,
	'project.managers': said,
	'request.submit': said,
	'request.update': said,
	'request.withdraw': said,
	'request.approve': said,
	'request.reject': said,
	'request.changesRequested': said,
	'role.grant': said,
	'role.revoke': said,
	'verification.grant': said,
	'verification.revoke': said,
	'booking.create': said,
	'booking.message': said,
	'booking.status': said,
	'planner.save': said,
	'planner.unsave': said,
	'planner.show': said,
	'epk.save': said,
	'epk.delete': said,
	'epk.request': said,
	'epk.grant': said,
	'epk.decline': (summary) => said(summary).replace('a request to see', 'someone asking to see'),
	'epk.revoke': said,
	'epk.forget': said,
	'show.save': said,
	'show.delete': said,
	'follow.create': said,
	'follow.delete': said,
	'profile.save': said,
	'space.create': said,
	'people.add': said,
	'people.update': said,
	'people.remove': said,
	'people.leave': words('left a group'),
	'people.hide': said,
	'people.invite': said,
	'people.join': words('joined a group'),
	'people.request': words('asked to join a group'),
	'people.approve': said,
	'people.reject': said,
	'people.link': said,
	'listing.list': said,
	'listing.unlist': said,
	'flag.update': said,
	'flag.preview': said,
	'flagGroup.create': said,
	'flagGroup.update': said,
	'flagGroup.delete': said,
	// Entries written before these were called requests still say "a submission".
	'record.delete': (summary) =>
		said(summary)
			.replace('a submission', 'a request to staff')
			.replace('a press kit request', 'a press kit access request')
};

const isPhrased = (action: string): action is keyof typeof PHRASES => Object.hasOwn(PHRASES, action);

/**
 * An audit entry as a plain sentence, like "You followed Neon Tide". `who` names the actor, for a page that shows someone
 * else's entries. Anything without a phrase, an action from a newer version included, reads as its summary.
 */
export function auditSentence(
	{ action, summary, outcome }: Pick<AuditEntryRow, 'action' | 'summary' | 'outcome'>,
	who = 'You'
): string {
	if (outcome === 'denied') return `Not allowed: ${summary.replace(/^Denied: /, '')}`;
	// A copy recovered from the network can come back without a summary.
	const unsaid = 'Something changed';

	if (!isPhrased(action)) return summary || unsaid;
	const phrase = PHRASES[action](summary);

	if (!phrase) return unsaid;

	return `${who} ${who === 'You' ? phrase.replace(/\btheir (own|planner|profile|behalf)\b/, 'your $1') : phrase}`;
}

/** Where each copy of an entry stands, for the details of a row. */
export const AUDIT_COPY_LABEL: Record<CopyStatus, string> = {
	pending: 'Saving',
	written: 'Saved',
	verified: 'Saved and checked',
	failed: "Couldn't be saved",
	self: 'Not needed',
	unsupported: "Your account's server can't keep one"
};
