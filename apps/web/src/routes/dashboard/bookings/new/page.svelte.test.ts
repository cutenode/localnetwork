import { page } from 'vitest/browser';
import { afterEach, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { draftKey } from '#lib/form-draft.svelte.ts';
import { inquiryDefaults, inquiryValues } from '#lib/shows.ts';
import type { ProjectView, Viewer } from '#lib/types.ts';
import Page from './+page.svelte';

const project: ProjectView = {
	uri: 'at://did:plc:root/computer.localnetwork.project/2',
	rkey: '2',
	cid: 'c',
	slug: 'neon-tide',
	profile: { name: 'Neon Tide', kind: 'solo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } },
	managers: ['did:plc:neon'],
	status: 'active',
	revision: 1,
	createdBy: 'did:plc:a',
	createdAt: '2026-01-01T00:00:00Z',
	updatedAt: '2026-01-01T00:00:00Z',
	avatarUrl: null
};

const viewer: Viewer = {
	did: 'did:plc:pat',
	handle: 'pat.test',
	roles: [],
	isAdmin: false,
	isStaff: false,
	spacesCapable: true
};

const fromShow = {
	viewer,
	pendingRequests: 0,
	unread: 0,
	waiting: 0,
	needs: [],
	access: Promise.resolve([]),
	sections: Promise.resolve(null),
	projects: Promise.resolve([]),
	project,
	values: inquiryValues({ title: 'Rooftop Rave', date: '2026-12-12T23:30:00.000Z', venue: 'The Lot', capacity: 300 }),
	show: { rkey: '3kshow', title: 'Rooftop Rave' },
	asks: [],
	geocoder: false
};

const posted = () => new FormData(page.getByRole('button', { name: 'Send inquiry' }).element().closest('form')!);

const KEY = draftKey(viewer, `inquiry:${project.uri}:3kshow`)!;

afterEach(() => localStorage.removeItem(KEY));

it("starts from the planner show's details and links back to it", async () => {
	await render(Page, { params: {}, data: fromShow, form: null });

	await expect
		.element(page.getByRole('link', { name: 'Rooftop Rave' }))
		.toHaveAttribute('href', '/dashboard/planner/shows/3kshow');
	expect(Object.fromEntries(posted())).toMatchObject({
		title: 'Rooftop Rave',
		date: '2026-12-12',
		time: '23:30',
		venue: 'The Lot',
		capacity: '300',
		currency: 'USD'
	});
});

it('brings back an unsent inquiry to the same project from the same show', async () => {
	const first = await render(Page, { params: {}, data: fromShow, form: null });

	await page.getByLabelText('Message').fill('Would you close our rooftop night?');
	await page.getByLabelText('Venue').fill('The Roof');
	await expect.poll(() => localStorage.getItem(KEY)).toContain('The Roof');
	await first.unmount();

	await render(Page, { params: {}, data: fromShow, form: null });
	await expect.element(page.getByLabelText('Message')).toHaveValue('Would you close our rooftop night?');
	await expect.element(page.getByLabelText('Venue')).toHaveValue('The Roof');
	await expect.element(page.getByText(/Your unsent changes from .* are back\./)).toBeInTheDocument();
});

it('opens without a planner show, with only the defaults', async () => {
	await render(Page, { params: {}, data: { ...fromShow, values: inquiryDefaults, show: null }, form: null });

	await expect.element(page.getByText(/in your planner/)).not.toBeInTheDocument();
	expect(Object.fromEntries(posted())).toMatchObject({ title: '', time: '22:00', currency: 'USD' });
});

it('shows what the project asks for beside the budget, and leaves the venue city optional', async () => {
	await render(Page, {
		params: {},
		data: { ...fromShow, values: inquiryDefaults, show: null, asks: ['Fee: Established.', 'Slots: Peak time.'] },
		form: null
	});

	await expect
		.element(page.getByLabelText('Budget'))
		.toHaveAccessibleDescription('Fee: Established. Slots: Peak time.');
	await expect.element(page.getByLabelText('City')).not.toBeRequired();
});

it('keeps the venue city after a failed send, with nothing beside the budget for a project that asks nothing', async () => {
	await render(Page, {
		params: {},
		data: { ...fromShow, values: inquiryDefaults, show: null },
		form: {
			error: 'Give the show a name',
			values: { ...inquiryDefaults, city: 'Leipzig', country: 'DE' },
			reauth: false
		}
	});

	await expect.element(page.getByLabelText('City')).toHaveValue('Leipzig');
	await expect.element(page.getByLabelText('Budget')).not.toHaveAttribute('aria-describedby');
});
