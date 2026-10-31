import { createRawSnippet } from 'svelte';
import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { ProjectView, Viewer } from '#lib/types.ts';
import Layout from './+layout.svelte';

const children = createRawSnippet(() => ({ render: () => '<p>Page</p>' }));

const viewer = (isStaff: boolean): Viewer => ({
	did: 'did:plc:neon',
	handle: 'neon.test',
	roles: isStaff ? ['moderator'] : [],
	isAdmin: false,
	isStaff,
	spacesCapable: true
});

// SAFETY: the sidebar reads only a project's slug and name.
const neon = {
	uri: 'at://did:plc:root/computer.localnetwork.project/2',
	slug: 'neon-tide',
	profile: { name: 'Neon Tide' }
} as ProjectView;

const menu = async () => {
	await page.getByRole('button', { name: /^Dashboard menu/ }).click();

	return page.getByRole('navigation', { name: 'Dashboard' });
};

it('lists the sections, each project the viewer manages, and the way back to the site', async () => {
	await render(Layout, {
		params: {},
		data: { viewer: viewer(false), pendingRequests: 0, unread: 0, projects: Promise.resolve([neon]) },
		children
	});
	const nav = await menu();

	for (const [name, href] of [
		['Overview', '/dashboard'],
		['Inbox', '/dashboard/inbox'],
		['Bookings', '/dashboard/bookings'],
		['Planner', '/dashboard/planner'],
		['Requests', '/dashboard/requests'],
		['Activity', '/dashboard/activity'],
		['View site', '/']
	]) {
		await expect.element(nav.getByRole('link', { name, exact: true })).toHaveAttribute('href', href);
	}

	const projects = nav.getByRole('list', { name: 'Projects' });
	await expect
		.element(projects.getByRole('link', { name: 'Neon Tide' }))
		.toHaveAttribute('href', '/dashboard/projects/neon-tide');
	await expect
		.element(projects.getByRole('link', { name: 'List a project' }))
		.toHaveAttribute('href', '/dashboard/projects/new');
	await expect.element(nav.getByRole('link', { name: /Staff console/ })).not.toBeInTheDocument();
});

it('adds the staff console, with the requests waiting, for staff', async () => {
	await render(Layout, {
		params: {},
		data: { viewer: viewer(true), pendingRequests: 4, unread: 0, projects: Promise.resolve([]) },
		children
	});
	const nav = await menu();

	await expect.element(nav.getByRole('link', { name: 'Staff console 4' })).toHaveAttribute('href', '/admin');
});

it('counts what is unread in the inbox', async () => {
	await render(Layout, {
		params: {},
		data: { viewer: viewer(false), pendingRequests: 0, unread: 3, projects: Promise.resolve([]) },
		children
	});
	const nav = await menu();

	await expect.element(nav.getByRole('link', { name: 'Inbox 3' })).toHaveAttribute('href', '/dashboard/inbox');
});

it('still offers to list a project when the directory is offline', async () => {
	await render(Layout, {
		params: {},
		data: { viewer: viewer(false), pendingRequests: 0, unread: 0, projects: Promise.resolve(null) },
		children
	});
	const nav = await menu();

	const projects = nav.getByRole('list', { name: 'Projects' });
	await expect.element(projects.getByRole('link', { name: 'List a project' })).toBeInTheDocument();
	expect(projects.getByRole('link').elements()).toHaveLength(1);
});
