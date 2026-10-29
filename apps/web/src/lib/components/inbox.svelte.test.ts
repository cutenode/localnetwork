import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { NotificationView } from '#lib/types.ts';
import InboxList from './InboxList.svelte';

const item = (id: string, overrides: Partial<NotificationView> = {}): NotificationView => ({
	id,
	topic: 'bookings',
	title: 'Neon Tide sent a message',
	body: 'Warehouse night',
	href: '/dashboard/bookings/3mabc',
	occurredAt: new Date(Date.now() - 2 * 3_600_000).toISOString(),
	read: false,
	...overrides
});

it('opens each item through a form that marks it read, saying which are unread', async () => {
	await render(InboxList, {
		items: [item('a'), item('b', { title: 'New booking inquiry for Neon Tide', body: undefined, read: true })]
	});

	const unread = page.getByRole('button', { name: 'Neon Tide sent a message Warehouse night 2 hours ago Unread' });
	await expect.element(unread).toHaveAttribute('type', 'submit');
	await expect
		.element(page.getByRole('button', { name: 'New booking inquiry for Neon Tide 2 hours ago' }))
		.toBeVisible();

	const forms = page
		.getByRole('listitem')
		.elements()
		.map((listed) => listed.querySelector('form'));

	expect(forms.map((form) => [form?.getAttribute('action'), new FormData(form ?? undefined).get('id')])).toEqual([
		['?/open', 'a'],
		['?/open', 'b']
	]);
});

it('says what will show up here when nothing has yet', async () => {
	await render(InboxList, { items: [] });

	await expect.element(page.getByText('Nothing yet')).toBeVisible();
	await expect.element(page.getByText('Booking inquiries and messages show up here.')).toBeVisible();
	await expect
		.element(page.getByRole('link', { name: 'Get them by email too' }))
		.toHaveAttribute('href', '/dashboard/inbox/settings');
	expect(page.getByRole('button').elements()).toHaveLength(0);
});
