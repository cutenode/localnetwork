import { page } from 'vitest/browser';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { Viewer } from '#lib/types.ts';
import SiteHeader from '#lib/ui/SiteHeader.svelte';
import { siteNav } from './nav.ts';

const viewer = (isStaff: boolean): Viewer => ({
	did: 'did:plc:neon',
	handle: 'neon.test',
	roles: isStaff ? ['moderator'] : [],
	isAdmin: false,
	isStaff,
	spacesCapable: true
});

/** The header as the layout draws it on `path` for this viewer. */
const show = (path: string, data: { viewer: Viewer | null; pendingRequests: number }) =>
	render(SiteHeader, { ...siteNav(data.viewer, data.pendingRequests), current: path });

const header = () => page.getByRole('banner');

beforeEach(() => page.viewport(1280, 800));

// Back to the size the other tests expect.
afterEach(() => page.viewport(414, 896));

describe('the site header', () => {
	it('links the four destinations, For DJs and Sign in for someone signed out', async () => {
		await show('/', { viewer: null, pendingRequests: 0 });
		const main = header().getByRole('navigation', { name: 'Main' });

		expect(
			main
				.getByRole('link')
				.elements()
				.map((link) => [link.textContent, link.getAttribute('href')])
		).toEqual([
			['Find DJs', '/projects'],
			['Shows', '/shows'],
			['Charts', '/charts'],
			['Tools', '/tools']
		]);
		await expect.element(header().getByRole('link', { name: 'For DJs' })).toHaveAttribute('href', '/for-djs');
		await expect.element(header().getByRole('link', { name: 'Sign in' })).toHaveAttribute('href', '/login');
		await expect.element(header().getByRole('link', { name: 'Dashboard' })).not.toBeInTheDocument();
		await expect.element(header().getByRole('button', { name: /^Account/ })).not.toBeInTheDocument();
	});

	it('swaps them for Dashboard and an account menu once signed in', async () => {
		await show('/', { viewer: viewer(false), pendingRequests: 0 });

		await expect.element(header().getByRole('link', { name: 'Dashboard' })).toHaveAttribute('href', '/dashboard');
		await expect.element(header().getByRole('link', { name: 'Sign in' })).not.toBeInTheDocument();
		await expect.element(header().getByRole('link', { name: 'For DJs' })).not.toBeInTheDocument();
		await expect.element(header().getByRole('link', { name: 'List your project' })).not.toBeInTheDocument();
		await expect.element(header().getByRole('link', { name: 'Bookings' })).not.toBeInTheDocument();
		await header().getByRole('button', { name: 'Account, @neon.test' }).click();
		await expect.element(header().getByRole('button', { name: 'Sign out' })).toBeVisible();
		await expect.element(header().getByRole('link', { name: /Staff console/ })).not.toBeInTheDocument();
	});

	it('adds the staff console to the account menu for staff', async () => {
		await show('/', { viewer: viewer(true), pendingRequests: 0 });
		await header().getByRole('button', { name: 'Account, @neon.test' }).click();

		await expect
			.element(header().getByRole('link', { name: 'Staff console', exact: true }))
			.toHaveAttribute('href', '/admin');
	});

	it('flags the requests waiting for staff on the account button and the staff console', async () => {
		await show('/', { viewer: viewer(true), pendingRequests: 4 });
		await header().getByRole('button', { name: 'Account, @neon.test, 4 requests waiting' }).click();

		await expect.element(header().getByRole('link', { name: 'Staff console 4' })).toHaveAttribute('href', '/admin');
	});

	it.each([
		['/projects', 'Find DJs'],
		['/projects/neon-tide', 'Find DJs'],
		['/match', 'Find DJs'],
		['/shows', 'Shows'],
		['/artists/okon', 'Charts'],
		['/tools/locallibrary', 'Tools']
	])('marks %s as under %s', async (path, name) => {
		await show(path, { viewer: null, pendingRequests: 0 });

		await expect.element(header().getByRole('link', { name, exact: true })).toHaveAttribute('aria-current', 'page');
		expect(header().element().querySelectorAll('[aria-current]')).toHaveLength(1);
	});

	it('marks only Dashboard current on the pages under it', async () => {
		await show('/dashboard/bookings', { viewer: viewer(true), pendingRequests: 0 });

		await expect.element(header().getByRole('link', { name: 'Dashboard' })).toHaveAttribute('aria-current', 'page');
		expect(header().element().querySelectorAll('[aria-current]')).toHaveLength(1);
	});
});
