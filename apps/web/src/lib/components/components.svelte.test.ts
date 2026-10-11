import { page, userEvent } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { sampleReport } from '#lib/mix/sample.ts';
import Avatar from './Avatar.svelte';
import DiffTable from './DiffTable.svelte';
import GenreInput from './GenreInput.svelte';
import MixList from './MixList.svelte';
import MixReport from './mix/MixReport.svelte';
import PlacePicker from './PlacePicker.svelte';
import ProjectCard from './ProjectCard.svelte';
import ProjectForm from './ProjectForm.svelte';
import ScoreBar from './ScoreBar.svelte';
import StatusPill from './StatusPill.svelte';

describe('StatusPill', () => {
	it('humanizes statuses', async () => {
		await render(StatusPill, { status: 'changes_requested' });
		await expect.element(page.getByText('changes requested')).toBeInTheDocument();
	});
});

describe('Avatar', () => {
	it('falls back to initials when there is no image', async () => {
		await render(Avatar, { name: 'Neon Tide', src: null });
		await expect.element(page.getByText('NT')).toBeInTheDocument();
	});
});

describe('ScoreBar', () => {
	it('shows the score', async () => {
		await render(ScoreBar, { score: 87 });
		await expect.element(page.getByTitle('Match score 87 of 100')).toBeInTheDocument();
	});
});

describe('DiffTable', () => {
	it('renders before and after values with readable field names', async () => {
		await render(DiffTable, { changes: [{ field: 'profile.location.city', before: 'Leeds', after: 'York' }] });
		await expect.element(page.getByText('location › city')).toBeInTheDocument();
		await expect.element(page.getByText('York')).toBeInTheDocument();
	});
	it('marks a missing before value as none', async () => {
		await render(DiffTable, { changes: [{ field: 'profile.description', after: 'Late-night jungle' }] });
		await expect.element(page.getByRole('cell', { name: 'none' })).toBeInTheDocument();
	});
	it('shows an empty message', async () => {
		await render(DiffTable, { changes: [], empty: 'Nothing changed' });
		await expect.element(page.getByText('Nothing changed')).toBeInTheDocument();
	});
});

describe('GenreInput', () => {
	it('turns typed genres into normalized chips and a hidden value', async () => {
		await render(GenreInput, { value: 'House' });
		await expect.element(page.getByText('house')).toBeInTheDocument();
		await page.getByRole('combobox').fill('DnB');
		await userEvent.keyboard('{Enter}');
		await expect.element(page.getByText('drum and bass')).toBeInTheDocument();
		expect(document.querySelector<HTMLInputElement>('input[type=hidden][name=genres]')?.value).toBe(
			'house, drum and bass'
		);
		await page.getByRole('button', { name: 'Remove house' }).click();
		expect(document.querySelector<HTMLInputElement>('input[type=hidden][name=genres]')?.value).toBe('drum and bass');
	});
});

describe('ProjectCard', () => {
	it('links to the project and marks unclaimed projects', async () => {
		await render(ProjectCard, {
			project: {
				uri: 'at://did:plc:root/computer.localnetwork.project/1',
				rkey: '1',
				cid: 'c',
				slug: 'neon-tide',
				profile: {
					name: 'Neon Tide',
					kind: 'solo',
					genres: ['techno', 'dub techno'],
					location: { city: 'Berlin', country: 'DE' },
					tagline: 'Hypnotic'
				},
				managers: [],
				status: 'active',
				revision: 1,
				createdBy: 'did:plc:a',
				createdAt: '2026-01-01T00:00:00Z',
				updatedAt: '2026-01-01T00:00:00Z',
				avatarUrl: null,
				distanceKm: 3.2
			}
		});
		await expect.element(page.getByRole('link', { name: 'Neon Tide' })).toHaveAttribute('href', '/projects/neon-tide');
		await expect.element(page.getByText('Unclaimed')).toBeInTheDocument();
		await expect.element(page.getByText('3.2 km')).toBeInTheDocument();
	});
});

describe('MixList', () => {
	it('renders mixes that repeat a link', async () => {
		const mix = { title: 'Tidal Hours', url: 'https://soundcloud.com/neon/tidal-hours' };
		await render(MixList, { mixes: [mix, { ...mix }] });
		await expect.element(page.getByRole('button', { name: 'Play' }).nth(1)).toBeInTheDocument();
	});
});

describe('MixReport', () => {
	it('renders a report and shares the selected track between views', async () => {
		await render(MixReport, { report: sampleReport });
		await expect.element(page.getByRole('heading', { level: 1, name: 'Sunday Warm-up' })).toBeInTheDocument();

		for (const name of ['The mix', 'Tempo', 'Key journey', 'Tracklist']) {
			await expect.element(page.getByRole('heading', { level: 2, name })).toBeInTheDocument();
		}

		const halogen = page.getByRole('button', { name: /^3\. Halogen by Two Rooms/ });
		await halogen.first().click();
		await expect.element(halogen.last()).toHaveAttribute('aria-pressed', 'true');
		await expect.element(page.getByText(/key clash/).first()).toBeInTheDocument();
	});
});

describe('ProjectForm', () => {
	it('posts back link labels and mix lengths and years it has no fields for', async () => {
		await render(ProjectForm, {
			values: {
				links: [{ kind: 'website', url: 'https://a.test', label: 'Shop' }],
				mixes: [{ title: 'One', url: 'https://m.test', durationMinutes: 90, year: 2024 }]
			},
			geocoder: false
		});

		const hidden = (name: string) =>
			document.querySelector<HTMLInputElement>(`input[type=hidden][name=${name}]`)?.value;

		expect([hidden('linkLabel'), hidden('mixDuration'), hidden('mixYear')]).toEqual(['Shop', '90', '2024']);
	});
});

describe('PlacePicker', () => {
	it('drops picked coordinates once the place is edited by hand', async () => {
		await render(PlacePicker, {
			values: { city: 'Berlin', country: 'DE', latE6: 52_520_000, lngE6: 13_405_000 },
			geocoder: false
		});
		const coordinate = (name: string) => document.querySelector<HTMLInputElement>(`input[name=${name}]`)?.value;
		expect([coordinate('latE6'), coordinate('lngE6')]).toEqual(['52520000', '13405000']);
		await page.getByLabelText('City').fill('Potsdam');
		expect([coordinate('latE6'), coordinate('lngE6')]).toEqual(['', '']);
		expect(document.querySelector('p.hint')?.textContent).toMatch(/Pick a city/);
	});
});
