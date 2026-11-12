import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import Page from './+page.svelte';
import type { PageProps } from './$types';

type Strays = PageProps['data']['strays'];

const renderWith = (strays: Strays) =>
	render(Page, {
		params: {},
		form: null,
		data: {
			viewer: {
				did: 'did:plc:admin',
				handle: 'admin.test',
				roles: ['admin'],
				isAdmin: true,
				isStaff: true,
				spacesCapable: true
			},
			pendingRequests: 0,
			staff: [],
			strays
		}
	});

describe('the staff page', () => {
	it('shows who a removal left behind once that arrives, without waiting for it', async () => {
		let arrive = (_strays: Awaited<Strays>) => {};

		await renderWith(
			new Promise((resolve) => {
				arrive = resolve;
			})
		);

		await expect.element(page.getByRole('heading', { name: 'Add staff' })).toBeVisible();
		await expect.element(page.getByRole('heading', { name: 'Still in private spaces' })).not.toBeInTheDocument();

		arrive([{ did: 'did:plc:mod', handle: 'mod.test', spaces: ['audit'] }]);

		await expect.element(page.getByRole('link', { name: '@mod.test' })).toBeVisible();
	});

	it('names each role in a word and says what the chosen one can do under the select', async () => {
		await renderWith(Promise.resolve([]));
		const role = page.getByRole('combobox', { name: 'Role' });
		const options = page.getByRole('option').elements();

		expect(options.map((option) => option.textContent)).toEqual(['Moderator', 'Admin']);
		await expect
			.element(role)
			.toHaveAccessibleDescription('Reviews requests, hides projects, and reads the audit log.');

		await role.selectOptions('Admin');

		await expect.element(role).toHaveAccessibleDescription('Everything, including staff and direct edits.');
	});
});
