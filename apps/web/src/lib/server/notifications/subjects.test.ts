import { describe, expect, it } from 'vitest';
import type { NotificationRow } from '../db/schema';
import { summarize } from './subjects';

const row = (
	id: string,
	kind: string,
	data: Record<string, string | null>,
	readAt: string | null = null
): NotificationRow => ({
	id,
	recipientDid: 'did:plc:neon',
	kind,
	dedupeKey: id,
	subjectUri: 'at://space/1',
	href: '/dashboard/bookings/3k',
	data: JSON.stringify(data),
	occurredAt: `2026-11-0${id}T00:00:00.000Z`,
	createdAt: `2026-11-0${id}T00:00:00.000Z`,
	readAt
});

const inquiry = { skey: '3k', title: null, from: 'promoter', projectName: null };

describe('summarize', () => {
	it('counts the same thing done twice, and falls back to the thread kind when it has no title or act', () => {
		const view = summarize([row('2', 'booking.inquiry', inquiry), row('1', 'booking.inquiry', inquiry)]);

		expect(view).toMatchObject({
			title: 'Booking inquiry',
			scope: 'Booking inquiry',
			summary: 'The promoter sent 2 booking inquiries',
			topic: 'bookings',
			unread: true
		});
	});

	it('words items a later version dropped generically, with nobody named and no topic', () => {
		expect(summarize([row('2', 'made.up', {}), row('1', 'made.up', {})])).toMatchObject({
			title: 'Something changed',
			scope: 'Local Network',
			summary: '2 things changed',
			topic: null
		});
	});

	it('says only the newest thing once everything is read', () => {
		const read = '2026-11-05T00:00:00.000Z';

		expect(summarize([row('2', 'made.up', {}, read), row('1', 'booking.inquiry', inquiry, read)])).toMatchObject({
			title: 'Booking inquiry',
			summary: 'Something changed',
			unread: false
		});
	});
});
