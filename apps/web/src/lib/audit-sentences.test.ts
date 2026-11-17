import { describe, expect, it } from 'vitest';
import { AUDIT_ACTIONS } from '#lib/server/audit/log.ts';
import { auditSentence } from './audit-sentences.ts';

const said = (action: string, summary: string, outcome = 'success') => auditSentence({ action, summary, outcome });

describe('auditSentence', () => {
	it("carries the entry's own words on after the actor, so it still names the act or show", () => {
		expect(said('follow.create', 'Followed Neon Tide')).toBe('You followed Neon Tide');
		expect(said('show.save', 'Announced the show “Friday Social” for Neon Tide')).toBe(
			'You announced the show “Friday Social” for Neon Tide'
		);
	});

	it('uses fixed words where the summary adds nothing a person needs', () => {
		expect(said('auth.login', 'Signed in as @neon.test')).toBe('You signed in');
		expect(said('people.leave', 'Left')).toBe('You left a group');
	});

	it('speaks to the viewer about their own things, and of someone else as themselves', () => {
		const entry = { action: 'profile.save', summary: 'Updated their profile', outcome: 'success' };

		expect(auditSentence(entry)).toBe('You updated your profile');
		expect(auditSentence(entry, '@pat.test')).toBe('@pat.test updated their profile');
		expect(said('show.save', 'Announced the show “All their love” for Neon Tide')).toContain('All their love');
		expect(said('flag.preview', 'Previewed dashboardCalendar on in their own browser')).toBe(
			'You previewed dashboardCalendar on in your own browser'
		);
	});

	it('drops revision numbers', () => {
		expect(said('project.update', 'Edited Neon Tide (revision 4)')).toBe('You edited Neon Tide');
		expect(said('project.update', 'Updated Neon Tide to revision 4')).toBe('You updated Neon Tide');
	});

	it('rewords entries written before the words changed', () => {
		expect(said('record.delete', 'Deleted a submission filed on their behalf')).toBe(
			'You deleted a request to staff filed on your behalf'
		);
		expect(said('record.delete', 'Deleted a press kit request filed on their behalf')).toBe(
			'You deleted a press kit access request filed on your behalf'
		);
		expect(said('epk.decline', 'Declined a request to see the press kit “Clubs”')).toBe(
			'You declined someone asking to see the press kit “Clubs”'
		);
	});

	it('says when something was refused', () => {
		expect(said('booking.status', 'Denied: only the other side can confirm this', 'denied')).toBe(
			'Not allowed: only the other side can confirm this'
		);
	});

	it('reads an action it has no words for as its summary, and copes without one', () => {
		expect(said('made.up', 'Something a newer version wrote')).toBe('Something a newer version wrote');
		expect(said('epk.view', 'Someone with access viewed the press kit “Clubs”')).toBe(
			'Someone with access viewed the press kit “Clubs”'
		);
		expect(said('made.up', '')).toBe('Something changed');
		expect(said('follow.create', '')).toBe('Something changed');
	});

	it('has an answer for every action the log records', () => {
		for (const action of AUDIT_ACTIONS) expect(said(action, 'Did a thing')).toMatch(/\S/);

		expect(
			AUDIT_ACTIONS.filter((action) => !said(action, 'Did a thing').startsWith('You ')),
			'only these stand on their own'
		).toEqual(['epk.view', 'system.bootstrap', 'system.space']);
	});
});
