import { page } from 'vitest/browser';
import { afterEach, beforeEach, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import AccountNav from './AccountNav.svelte';

beforeEach(() => page.viewport(1280, 800));

// Back to the size the other tests expect.
afterEach(() => page.viewport(414, 896));

const pages = () => page.getByRole('navigation', { name: 'Account pages' });

it('lists the area and marks where you are, on the pages under one too', async () => {
	await render(AccountNav, { path: '/dashboard/account/requests/3k' });

	expect(
		pages()
			.getByRole('link')
			.elements()
			.map((link) => [link.textContent, link.getAttribute('href')])
	).toEqual([
		['Account', '/dashboard/account'],
		['Notifications', '/dashboard/account/notifications'],
		['Activity', '/dashboard/account/activity'],
		['Your data', '/dashboard/account/data'],
		['Requests to staff', '/dashboard/account/requests']
	]);
	await expect
		.element(pages().getByRole('link', { name: 'Requests to staff' }))
		.toHaveAttribute('aria-current', 'page');
});

it('leaves the landing page to its own list', async () => {
	await render(AccountNav, { path: '/dashboard/account' });

	await expect.element(page.getByRole('link')).not.toBeInTheDocument();
});

it('steps back to Account from a page, and to its page from under it', async () => {
	const { rerender } = await render(AccountNav, { path: '/dashboard/account/activity' });
	const back = page.getByRole('link').first();

	await expect.element(back).toHaveTextContent('Account');
	await expect.element(back).toHaveAttribute('href', '/dashboard/account');

	await rerender({ path: '/dashboard/account/data/booking' });
	await expect.element(back).toHaveTextContent('Your data');
	await expect.element(back).toHaveAttribute('href', '/dashboard/account/data');
});
