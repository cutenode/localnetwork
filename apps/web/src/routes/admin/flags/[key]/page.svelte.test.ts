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

/** The signed-out switch, whose label also holds its hint. */
const SIGNED_OUT = /^Signed-out visitors/;

it('counts the percentage in accounts', async () => {
	await renderFlag({ percentage: 25 });

	await expect.element(page.getByLabelText('Percentage of accounts')).toHaveValue(25);
});

it('starts signed-out visitors off for a release flag, and holds the switch until every account has it', async () => {
	await renderFlag({ key: 'socialHome', type: 'release', percentage: 99 });

	const signedOut = page.getByRole('checkbox', { name: SIGNED_OUT });

	await expect.element(signedOut).not.toBeChecked();
	await expect.element(signedOut).toBeDisabled();
	await expect.element(signedOut).toHaveAccessibleDescription("Only once it's on for every account.");
});

it('lets staff turn signed-out visitors on once every account has the flag', async () => {
	await renderFlag({ key: 'socialHome', type: 'release', percentage: 100 });

	await expect.element(page.getByRole('checkbox', { name: SIGNED_OUT })).toBeEnabled();
});

it('says signed-out visitors go when the flag drops below every account', async () => {
	await renderFlag({ key: 'socialHome', type: 'release', everyone: true, signedOut: true });

	const signedOut = page.getByRole('checkbox', { name: SIGNED_OUT });

	await expect.element(signedOut).toBeChecked();
	await expect.element(signedOut).toHaveAccessibleDescription('Turns off if it drops below every account.');
});

it('gives an ops flag no signed-out switch, since everyone takes them in', async () => {
	await renderFlag({ key: 'pauseRecordingStream', type: 'ops' });

	await expect
		.element(page.getByRole('checkbox', { name: /^On for everyone/ }))
		.toHaveAccessibleDescription('Signed-out visitors too.');
	await expect.element(page.getByRole('checkbox', { name: SIGNED_OUT })).not.toBeInTheDocument();
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

it('says to save first when the form reaches every account but the saved flag does not', async () => {
	await renderFlag({ key: 'socialHome', type: 'release', percentage: 60 });

	await page.getByRole('checkbox', { name: /^On for everyone/ }).click();

	await expect.element(page.getByRole('checkbox', { name: SIGNED_OUT })).toBeDisabled();
	await expect
		.element(page.getByRole('checkbox', { name: SIGNED_OUT }))
		.toHaveAccessibleDescription('Save first, then this opens.');
});

// Helpers

/** The flag page for parityCanary, or the flag `fields` names, on top of a flag nobody has set. */
function renderFlag(fields: Partial<FlagView>) {
	const flag: FlagView = {
		key: 'parityCanary',
		description: 'Gates nothing.',
		type: 'ops',
		showsOn: [],
		enabled: true,
		signedOut: false,
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
