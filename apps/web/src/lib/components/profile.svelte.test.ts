import { page } from 'vitest/browser';
import type { z } from 'zod';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { DEMO_DJ_PROFILE, DEMO_DJ_SETS } from '../../../scripts/seed-data.ts';
import { djProfileSchema, djSetSchema } from '#lib/dj-schema.ts';
import type { ProjectProfile } from '#lib/types.ts';
import BookingDetails from './BookingDetails.svelte';
import DjSets from './DjSets.svelte';
import DjStats from './DjStats.svelte';
import ProjectFacts from './ProjectFacts.svelte';

const profile: ProjectProfile = {
	name: 'Neon Tide',
	kind: 'solo',
	genres: ['techno'],
	location: { city: 'Berlin', country: 'DE' }
};

describe('DjStats', () => {
	it('shows only the sections a profile has', async () => {
		await render(DjStats, {
			profile: djProfileSchema.parse({
				generatedAt: '2026-10-01T00:00:00Z',
				tempo: { low: 120, high: 124, median: 122 }
			})
		});

		await expect.element(page.getByText(/120–124 BPM/)).toBeInTheDocument();
		await expect.element(page.getByText('Sets on file')).not.toBeInTheDocument();
		await expect.element(page.getByText('How they mix')).not.toBeInTheDocument();
		await expect.element(page.getByText('Energy')).not.toBeInTheDocument();
		await expect.element(page.getByText('Signature tracks')).not.toBeInTheDocument();
	});

	it('shows the rest when they are there', async () => {
		await render(DjStats, { profile: djProfileSchema.parse(DEMO_DJ_PROFILE) });

		await expect.element(page.getByText('How they mix')).toBeInTheDocument();
		await expect.element(page.getByText('Keys played most')).toBeInTheDocument();
		await expect.element(page.getByText('Set shape')).toBeInTheDocument();
		await expect.element(page.getByText('Signature tracks')).toBeInTheDocument();
	});
});

describe('DjSets', () => {
	const set = (value: z.input<typeof djSetSchema>, uri: string) => ({ ...djSetSchema.parse(value), uri });

	it('names a set by where it was when it has no title, and keeps to the limit', async () => {
		await render(DjSets, {
			sets: [
				set({ date: '2026-09-26', venue: 'Halle Nord', tracks: [{ title: 'Night Bus', startSeconds: 65 }] }, 'at://a'),
				set({ date: '2026-09-12', tracks: [{ unidentified: true }] }, 'at://b'),
				set(DEMO_DJ_SETS[0], 'at://c')
			],
			limit: 2
		});

		await expect.element(page.getByText('Halle Nord', { exact: true })).toBeInTheDocument();
		await expect.element(page.getByText('Untitled set')).toBeInTheDocument();
		await expect.element(page.getByText('1:05')).toBeInTheDocument();
		await expect.element(page.getByText('Unreleased')).toBeInTheDocument();
		await expect.element(page.getByText('Tidal Hours', { exact: true })).not.toBeInTheDocument();
	});
});

describe('ProjectFacts', () => {
	it('lists only the facts on file', async () => {
		await render(ProjectFacts, { profile: { ...profile, travel: { radiusKm: 0 }, capacity: { max: 300 } } });

		await expect.element(page.getByText('Local only')).toBeInTheDocument();
		await expect.element(page.getByText('0–300 people')).toBeInTheDocument();
		await expect.element(page.getByText('Fee')).not.toBeInTheDocument();
	});

	it('lists every fee level, led by what it covers', async () => {
		await render(ProjectFacts, {
			profile: {
				...profile,
				fees: [
					{ label: 'Club nights', tier: 'established', currency: 'EUR', minAmount: 600, maxAmount: 1500 },
					{ tier: 'community' }
				]
			}
		});

		await expect.element(page.getByRole('term')).toHaveTextContent('Fees');
		expect(
			page
				.getByRole('definition')
				.elements()
				.map((line) => line.textContent)
		).toEqual(['Club nights: Established · €600–€1,500', 'Community']);
	});

	it('shows the one fee of a record written before fee levels', async () => {
		await render(ProjectFacts, { profile: { ...profile, fee: { tier: 'emerging' } } });

		await expect.element(page.getByRole('term')).toHaveTextContent('Fee');
		await expect.element(page.getByRole('definition')).toHaveTextContent('Emerging');
	});

	it('leaves out the list when there is nothing to show', async () => {
		await render(ProjectFacts, { profile });

		expect(page.getByRole('term').elements()).toHaveLength(0);
	});
});

describe('BookingDetails', () => {
	const props = { details: {}, claimed: true };

	it('points everyone else to the press kits, where the details come with access', async () => {
		await render(BookingDetails, { ...props, details: null, kitsHref: '/projects/neon/press-kits' });

		await expect.element(page.getByText(/come with a press kit/)).toBeInTheDocument();
		await expect
			.element(page.getByRole('link', { name: 'See the press kits' }))
			.toHaveAttribute('href', '/projects/neon/press-kits');
	});

	it('shows what is on file to someone who may see it', async () => {
		await render(BookingDetails, { ...props, details: { bookingEmail: 'neon@example.com', techRider: '2x CDJ-3000' } });

		await expect
			.element(page.getByRole('link', { name: 'neon@example.com' }))
			.toHaveAttribute('href', 'mailto:neon@example.com');
		await expect.element(page.getByText('2x CDJ-3000')).toBeInTheDocument();
		await expect.element(page.getByText('Phone')).not.toBeInTheDocument();
	});

	it('points to an inquiry when nothing is on file', async () => {
		await render(BookingDetails, { ...props, details: { bookingEmail: '' } });

		await expect.element(page.getByText(/No private booking details yet\. Send an inquiry/)).toBeInTheDocument();
	});

	it('says only that nothing is on file for an unclaimed project', async () => {
		await render(BookingDetails, { ...props, claimed: false });

		await expect.element(page.getByText('No private booking details yet.')).toBeInTheDocument();
	});
});
