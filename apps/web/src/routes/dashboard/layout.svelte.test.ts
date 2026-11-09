import { createRawSnippet } from 'svelte';
import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { cleanup, render } from 'vitest-browser-svelte';
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
	await page.getByRole('button', { name: 'Dashboard menu' }).click();

	return page.getByRole('dialog', { name: 'Dashboard menu' });
};

it("lists the sections and everything that is the viewer's own", async () => {
	await render(Layout, {
		params: {},
		data: {
			viewer: viewer(false),
			pendingRequests: 0,
			unread: 0,
			projects: Promise.resolve([neon]),
			sections: Promise.resolve(null)
		},
		children
	});
	const nav = await menu();

	for (const [name, href] of [
		['Home', '/dashboard'],
		['Inbox', '/dashboard/inbox'],
		['Bookings', '/dashboard/bookings'],
		['Planner', '/dashboard/planner'],
		['Submissions', '/dashboard/requests'],
		['Activity', '/dashboard/activity'],
		['Your data', '/dashboard/data']
	]) {
		await expect.element(nav.getByRole('link', { name, exact: true })).toHaveAttribute('href', href);
	}

	const yours = nav.getByRole('list', { name: 'Yours' });
	await expect
		.element(yours.getByRole('link', { name: 'You', exact: true }))
		.toHaveAttribute('href', '/dashboard/profile');
	await expect
		.element(yours.getByRole('link', { name: 'Neon Tide' }))
		.toHaveAttribute('href', '/dashboard/projects/neon-tide');
	await expect
		.element(yours.getByRole('link', { name: 'List a project' }))
		.toHaveAttribute('href', '/dashboard/projects/new');
	await expect.element(nav.getByRole('link', { name: /Staff console/ })).not.toBeInTheDocument();
	await expect.element(nav.getByRole('link', { name: 'View site' })).not.toBeInTheDocument();
});

it('adds the staff console, with the requests waiting, for staff', async () => {
	await render(Layout, {
		params: {},
		data: {
			viewer: viewer(true),
			pendingRequests: 4,
			unread: 0,
			projects: Promise.resolve([]),
			sections: Promise.resolve(null)
		},
		children
	});
	const nav = await menu();

	await expect.element(nav.getByRole('link', { name: 'Staff console 4' })).toHaveAttribute('href', '/admin');
});

it('marks the inbox with a dot, not a count, while something is unread', async () => {
	await render(Layout, {
		params: {},
		data: {
			viewer: viewer(false),
			pendingRequests: 0,
			unread: 3,
			projects: Promise.resolve([]),
			sections: Promise.resolve(null)
		},
		children
	});
	const nav = await menu();

	await expect.element(nav.getByRole('link', { name: 'Inbox Unread' })).toHaveAttribute('href', '/dashboard/inbox');
});

it('still offers to list a project when the directory is offline', async () => {
	await render(Layout, {
		params: {},
		data: {
			viewer: viewer(false),
			pendingRequests: 0,
			unread: 0,
			projects: Promise.resolve(null),
			sections: Promise.resolve(null)
		},
		children
	});
	const nav = await menu();

	const yours = nav.getByRole('list', { name: 'Yours' });
	await expect.element(yours.getByRole('link', { name: 'List a project' })).toBeInTheDocument();
	expect(yours.getByRole('link').elements()).toHaveLength(2);
});

it('shows a DJ their projects and a promoter their planner, by the sections their profile has on', async () => {
	const open = async (sections: { performer: boolean; promoter: boolean }, projects: ProjectView[] = []) => {
		cleanup();

		await render(Layout, {
			params: {},
			data: {
				viewer: viewer(false),
				pendingRequests: 0,
				unread: 0,
				projects: Promise.resolve(projects),
				sections: Promise.resolve(sections)
			},
			children
		});

		return menu();
	};

	let nav = await open({ performer: false, promoter: true });
	await expect.element(nav.getByRole('link', { name: 'You', exact: true })).toBeInTheDocument();
	await expect.element(nav.getByRole('link', { name: 'Planner' })).toBeInTheDocument();
	await expect.element(nav.getByRole('link', { name: 'List a project' })).not.toBeInTheDocument();

	nav = await open({ performer: true, promoter: false });
	await expect.element(nav.getByRole('link', { name: 'List a project' })).toBeInTheDocument();
	await expect.element(nav.getByRole('link', { name: 'Planner' })).not.toBeInTheDocument();

	// Projects someone manages stay, whatever their profile says.
	nav = await open({ performer: false, promoter: false }, [neon]);
	await expect.element(nav.getByRole('link', { name: 'Neon Tide' })).toBeInTheDocument();
});
