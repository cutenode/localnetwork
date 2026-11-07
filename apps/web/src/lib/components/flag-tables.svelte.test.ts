import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { FlagGroupView, FlagView, StaffMember } from '#lib/types.ts';
import FlagGroupTable from './FlagGroupTable.svelte';
import FlagTable from './FlagTable.svelte';
import StaffTable from './StaffTable.svelte';

const flag = (gates: Partial<FlagView>): FlagView => ({
	key: 'parityCanary',
	description: 'Gates nothing.',
	everyone: false,
	percentage: 0,
	groups: [],
	accounts: [],
	updatedAt: null,
	...gates
});

const group = (fields: Partial<FlagGroupView>): FlagGroupView => ({
	rkey: '3lzy2ji4nms2z',
	name: 'Beta',
	description: null,
	members: [],
	flags: [],
	...fields
});

describe('FlagTable', () => {
	it.each([
		['off', {}, 'Off'],
		['on for everyone, whatever else is set', { everyone: true, percentage: 20 }, 'Everyone'],
		['a share of accounts', { percentage: 25 }, '25%'],
		[
			'one group and one account',
			{ groups: [{ rkey: 'a', name: 'Beta' }], accounts: [{ did: 'did:plc:a', handle: null }] },
			'1 group · 1 account'
		],
		[
			'several accounts',
			{
				accounts: [
					{ did: 'did:plc:a', handle: null },
					{ did: 'did:plc:b', handle: null }
				]
			},
			'2 accounts'
		]
	])('says who a flag is on for: %s', async (_, gates, expected) => {
		await render(FlagTable, { flags: [flag(gates)] });

		await expect.element(page.getByText(expected, { exact: true }).first()).toBeInTheDocument();
	});

	it('links each flag to its page, and says so when there are none', async () => {
		const { rerender } = await render(FlagTable, { flags: [flag({ updatedAt: '2026-11-02T10:00:00Z' })] });

		await expect
			.element(page.getByRole('link', { name: 'parityCanary' }).first())
			.toHaveAttribute('href', '/admin/flags/parityCanary');

		await rerender({ flags: [] });

		await expect.element(page.getByText('No flags yet.', { exact: false })).toBeInTheDocument();
	});
});

describe('FlagGroupTable', () => {
	it('shows who is in a group and which flags use it', async () => {
		await render(FlagGroupTable, {
			groups: [
				group({ description: 'Early', members: [{ did: 'did:plc:a', handle: 'a.test' }], flags: ['parityCanary'] })
			]
		});

		await expect
			.element(page.getByRole('link', { name: 'Beta' }).first())
			.toHaveAttribute('href', '/admin/flags/groups/3lzy2ji4nms2z');
		await expect.element(page.getByText('Early').first()).toBeInTheDocument();
		await expect.element(page.getByText('parityCanary').first()).toBeInTheDocument();
	});

	it('says when a group is unused, and when there are no groups', async () => {
		const { rerender } = await render(FlagGroupTable, { groups: [group({})] });

		await expect.element(page.getByText('No flags').first()).toBeInTheDocument();

		await rerender({ groups: [] });

		await expect.element(page.getByText('No groups yet.')).toBeInTheDocument();
	});
});

describe('StaffTable', () => {
	const member = (fields: Partial<StaffMember>): StaffMember => ({
		did: 'did:plc:mod',
		handle: 'mod.test',
		role: 'moderator',
		grants: [],
		grantedBy: 'did:plc:ada',
		note: null,
		createdAt: '2026-10-12T10:00:00Z',
		bootstrap: false,
		...fields
	});

	it('ticks the grants a moderator holds', async () => {
		await render(StaffTable, { staff: [member({ grants: ['flags'] })], viewer: 'did:plc:ada' });

		await expect.element(page.getByRole('checkbox', { name: 'Flags', exact: true }).first()).toBeChecked();
		await expect.element(page.getByRole('checkbox', { name: 'Flag groups' }).first()).not.toBeChecked();
	});

	it('offers an admin no grants, since they hold them all', async () => {
		await render(StaffTable, {
			staff: [member({ role: 'admin', grants: ['flags', 'flagGroups'] })],
			viewer: 'did:plc:ada'
		});

		await expect.element(page.getByRole('checkbox', { name: 'Flags', exact: true })).not.toBeInTheDocument();
	});
});
