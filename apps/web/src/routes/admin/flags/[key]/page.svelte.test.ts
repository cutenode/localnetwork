import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { ada } from '#lib/testing/requests.ts';
import type { FlagView } from '#lib/types.ts';
import Page from './+page.svelte';

it('lists the kinds of page the flag shows on', async () => {
	await renderFlag({ key: 'pauseRecordingStream', showsOn: ['set', 'mix'] });

	await expect.element(page.getByText('Shows on: Set, Mix', { exact: true })).toBeVisible();
});

it('names each kind as the staff console does, and every page for global', async () => {
	await renderFlag({ showsOn: ['pressKit', 'party'] });

	await expect.element(page.getByText('Shows on: Press kit, Party', { exact: true })).toBeVisible();

	await renderFlag({ showsOn: ['global'] });

	await expect.element(page.getByText('Shows on: Every page', { exact: true })).toBeVisible();
});

it('says so when the flag lists no kind of page', async () => {
	await renderFlag({ showsOn: [] });

	await expect.element(page.getByText('Shows on: None listed', { exact: true })).toBeVisible();
});

const CRAWLERS = 'Crawlers see it about this share of the time.';

it('warns that crawlers see a flag that shows on public pages about as often as its percentage', async () => {
	await renderFlag({ key: 'spacePages', showsOn: ['act', 'party', 'venue', 'event'] });

	await expect.element(page.getByText(CRAWLERS, { exact: false })).toBeVisible();
});

it('leaves crawlers out of the percentage hint for a flag that shows on no public page', async () => {
	await renderFlag({ showsOn: [] });

	await expect.element(page.getByText('anyone in stays in.', { exact: false })).toBeVisible();
	await expect.element(page.getByText(CRAWLERS, { exact: false })).not.toBeInTheDocument();
});

it("gives a release flag's remove-by date as its catalog file writes it", async () => {
	await renderFlag({ key: 'dashboardCalendar', removeBy: '2027-02-15' });

	await expect.element(page.getByText('Remove by: Feb 15, 2027', { exact: true })).toBeVisible();
});

it('gives no remove-by date for a flag without one', async () => {
	await renderFlag({});

	await expect.element(page.getByText('Shows on:', { exact: false })).toBeVisible();
	await expect.element(page.getByText('Remove by', { exact: false })).not.toBeInTheDocument();
});

it("links to the flag's changes in the audit log", async () => {
	await renderFlag({ key: 'dashboardCalendar' });

	await expect
		.element(page.getByRole('link', { name: 'Changes in the audit log' }))
		.toHaveAttribute('href', '/admin/audit?action=flag&q=dashboardCalendar');
});

it('says what to delete once the flag is ready to remove', async () => {
	await renderFlag({ key: 'dashboardCalendar', readyToRemove: true });

	await expect.element(page.getByText('Ready to remove', { exact: true })).toBeVisible();
	await expect
		.element(page.getByText('Delete flags/catalog/dashboardCalendar.json and the code it gates.'))
		.toBeVisible();
});

it("doesn't say to remove a flag that isn't ready", async () => {
	await renderFlag({ key: 'dashboardCalendar' });

	await expect.element(page.getByRole('heading', { name: 'dashboardCalendar', level: 1 })).toBeVisible();
	await expect.element(page.getByText('Ready to remove')).not.toBeInTheDocument();
});

// Helpers

/** The flag page for parityCanary, or the flag `fields` names, on top of a flag nobody has set. */
function renderFlag(fields: Partial<FlagView>) {
	const flag: FlagView = {
		key: 'parityCanary',
		description: 'Gates nothing.',
		showsOn: [],
		enabled: true,
		everyone: false,
		percentage: 0,
		groups: [],
		accounts: [],
		updatedAt: null,
		readyToRemove: false,
		...fields
	};

	return render(Page, {
		params: { key: flag.key },
		form: null,
		data: { viewer: ada, pendingRequests: 0, grants: ['flags'], flag, groups: [] }
	});
}
