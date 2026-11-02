import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { ada, neon, neonTide, request } from '#lib/testing/requests.ts';
import Page from './+page.svelte';

const base = {
	viewer: ada,
	pendingRequests: 1,
	project: neonTide,
	values: {},
	managers: [neon],
	trail: [],
	requests: [request('update', { status: 'approved' })],
	created: false,
	geocoder: false,
	sets: Promise.resolve([]),
	verifications: null
};

it("warns an admin before they publish over a manager's pending changes", async () => {
	await render(Page, { params: { slug: 'neon-tide' }, data: { ...base, requests: [request('update')] }, form: null });

	await expect.element(page.getByText(/has changes waiting for review/).getByText('@neon.test')).toBeInTheDocument();
	await expect
		.element(page.getByRole('link', { name: 'Review their changes' }))
		.toHaveAttribute('href', '/admin/requests/3kupdate');
	await expect.element(page.getByRole('link', { name: /Changes ·/ })).toBeInTheDocument();
});

it('has no warning once nothing is pending', async () => {
	await render(Page, { params: { slug: 'neon-tide' }, data: base, form: null });

	await expect.element(page.getByRole('button', { name: 'Save and publish' })).toBeInTheDocument();
	await expect.element(page.getByText(/has changes waiting for review/)).not.toBeInTheDocument();
});
