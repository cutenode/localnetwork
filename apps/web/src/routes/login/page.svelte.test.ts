import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import hostList from '#lib/hosts.json' with { type: 'json' };
import CreatePage from './create/+page.svelte';
import LoginPage from './+page.svelte';

const layout = { viewer: null, pendingRequests: 0 };

const login = { ...layout, next: '/projects/neon-tide', purpose: 'follow Neon Tide', error: null, reauth: false };

describe('/login', () => {
	it('says what signing in is for, links what an Atmosphere account is, and keeps `next` on the way to sign-up', async () => {
		await render(LoginPage, { params: {}, data: login, form: null });

		await expect.element(page.getByRole('heading', { level: 1, name: 'Hi there.' })).toBeVisible();
		await expect.element(page.getByText('Sign in with your Atmosphere account to follow Neon Tide.')).toBeVisible();
		await expect
			.element(page.getByRole('link', { name: 'Atmosphere account' }))
			.toHaveAttribute('href', 'https://atmosphereaccount.com');
		await expect
			.element(page.getByRole('link', { name: 'Make an account' }))
			.toHaveAttribute('href', '/login/create?next=%2Fprojects%2Fneon-tide');
		await expect.element(page.getByRole('button', { name: 'Sign in' })).toBeVisible();
	});

	it('falls back to what the site is for, and puts an unknown handle error on the field', async () => {
		await render(LoginPage, {
			params: {},
			data: { ...login, next: '/dashboard', purpose: null },
			form: {
				error: "We couldn't find @nobody.test or reach its host. Check the handle, or make an account.",
				handle: 'nobody.test'
			}
		});

		await expect
			.element(page.getByText('Sign in with your Atmosphere account to book DJs and manage your projects.'))
			.toBeVisible();
		const field = page.getByRole('combobox', { name: 'Your handle' });
		await expect.element(field).toHaveValue('nobody.test');
		await expect.element(field).toHaveAttribute('aria-invalid', 'true');
		await expect
			.element(field)
			.toHaveAccessibleDescription(
				"We couldn't find @nobody.test or reach its host. Check the handle, or make an account."
			);
		await expect.element(page.getByRole('link', { name: 'Make an account' })).toHaveAttribute('href', '/login/create');
	});

	it('explains a declined or failed sign-in, and asks a session with old permissions to sign in again', async () => {
		const { rerender } = await render(LoginPage, { params: {}, data: { ...login, error: 'denied' }, form: null });
		await expect
			.element(page.getByRole('alert'))
			.toHaveTextContent('You declined the request, so we could not sign you in.');

		await rerender({ data: { ...login, error: 'login' } });
		await expect.element(page.getByRole('alert')).toHaveTextContent('Sign-in was cancelled or failed. Try again.');

		await rerender({ data: { ...login, reauth: true } });
		await expect
			.element(page.getByText('Local Network needs new permissions. Sign in again to approve them.'))
			.toBeVisible();
	});
});

describe('/login/create', () => {
	it('lists every host as a button that starts sign-up there, and links back to sign-in', async () => {
		await render(CreatePage, {
			params: {},
			data: { ...layout, next: '/projects/neon-tide', hosts: hostList.hosts },
			form: null
		});

		await expect.element(page.getByRole('heading', { level: 1, name: 'Where should it live?' })).toBeVisible();

		for (const host of hostList.hosts) {
			await expect
				.element(page.getByRole('button', { name: new RegExp(`^${host.name}`) }))
				.toHaveAttribute('value', host.service);
		}

		await expect.element(page.getByText('Bluesky · most people start here')).toBeVisible();
		await expect
			.element(page.getByRole('link', { name: 'I already have an account' }))
			.toHaveAttribute('href', '/login?next=%2Fprojects%2Fneon-tide');
		await expect.element(page.getByText('You can move hosts later.')).toBeVisible();
	});

	it('says when a host could not be reached', async () => {
		await render(CreatePage, {
			params: {},
			data: { ...layout, next: '/dashboard', hosts: hostList.hosts },
			form: { error: "We couldn't reach Bluesky. Try again, or pick another host." }
		});

		await expect
			.element(page.getByRole('alert'))
			.toHaveTextContent("We couldn't reach Bluesky. Try again, or pick another host.");
		await expect
			.element(page.getByRole('link', { name: 'I already have an account' }))
			.toHaveAttribute('href', '/login');
	});
});
