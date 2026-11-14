import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { MigrationPlan as Plan } from '#lib/project-migration.ts';
import { migrationPlan, projectPlan } from '#lib/testing/migration-plan.ts';
import MigrationPlan from './MigrationPlan.svelte';

/** The card of the project called `name`. */
const card = (name: string) => page.getByRole('article').filter({ has: page.getByRole('heading', { name }) });

describe('MigrationPlan', () => {
	it('sums up the plan and what would be declined site-wide', async () => {
		await render(MigrationPlan, { plan: migrationPlan });

		await expect.element(page.getByText('2 acts · 2 DJs · 2 blocked')).toBeVisible();
		await expect.element(page.getByText(/1 create request would be declined/)).toBeVisible();
	});

	it('shows where each project goes, who moves with it, and what blocks it', async () => {
		await render(MigrationPlan, { plan: migrationPlan });

		await expect.element(card('Neon Tide').getByText('/projects/neon-tide/** → /@neon.test/**')).toBeVisible();
		await expect.element(card('Neon Tide').getByText('@neon.test (claimed as self)')).toBeVisible();
		await expect
			.element(card('Saffron Social').getByText('booking details, 0 drafts, 1 press kit, 1 booking thread'))
			.toBeVisible();
		await expect.element(card('Saffron Social').getByText('2 follows, 1 upcoming show')).toBeVisible();
		await expect.element(card('Moss & Marrow').getByText('Nobody manages it')).toBeVisible();
		await expect.element(card('Moss & Marrow').getByText('Unlisted')).toBeVisible();
		await expect.element(card('Pirate Signal').getByText('/projects/pirate-signal/** → nowhere yet')).toBeVisible();
		await expect.element(card('Pirate Signal').getByText(/^No DJ found/)).toBeVisible();
		await expect.element(card('Pirate Signal').getByText(/^A press kit is still in .*'s repo$/)).toBeVisible();
		await expect.element(card('Pirate Signal').getByText('claim request')).toBeVisible();
	});

	it('names taken slugs and DJ spaces, and people with no handle by their DID', async () => {
		const plan = {
			projects: [
				projectPlan('dupe', 'Dupe', {
					people: [{ did: 'did:plc:nohandle', handle: null, role: 'member', relationship: null }],
					blockers: [{ kind: 'slugTaken', slug: 'dupe', space: 'ats://taken' }]
				}),
				projectPlan('twice', 'Twice', {
					target: 'dj',
					dj: { did: 'did:plc:twice', handle: null, rule: 'override' },
					blockers: [{ kind: 'djSpaceTaken', did: 'did:plc:twice', space: 'ats://theirs' }]
				})
			],
			pendingCreates: [],
			totals: { acts: 1, djs: 1, blocked: 2 }
		} satisfies Plan;

		await render(MigrationPlan, { plan });

		await expect.element(card('Dupe').getByText('Another act already has /act/dupe')).toBeVisible();
		await expect.element(card('Dupe').getByText('did:plc:nohandle (member)')).toBeVisible();
		await expect.element(card('Twice').getByText('The DJ already has a space of their own')).toBeVisible();
		await expect.element(card('Twice').getByText('did:plc:twice (picked by staff)')).toBeVisible();
		await expect.element(page.getByText(/would be declined/)).not.toBeInTheDocument();
	});

	it('says when there are no projects', async () => {
		await render(MigrationPlan, {
			plan: { projects: [], pendingCreates: [], totals: { acts: 0, djs: 0, blocked: 0 } }
		});

		await expect.element(page.getByText('No projects to move')).toBeVisible();
	});
});
