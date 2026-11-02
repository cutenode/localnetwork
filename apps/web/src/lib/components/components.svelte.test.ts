import { page, userEvent } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { sampleReport } from '#lib/mix/sample.ts';
import type { DjSetView } from '#lib/types.ts';
import Avatar from './Avatar.svelte';
import ClaimFields from './ClaimFields.svelte';
import DiffTable from './DiffTable.svelte';
import GenreInput from './GenreInput.svelte';
import MixList from './MixList.svelte';
import MixReport from './mix/MixReport.svelte';
import PlacePicker from './PlacePicker.svelte';
import { djSummary, projectSummary } from '#lib/performers.ts';
import PerformerCard from './PerformerCard.svelte';
import ProjectForm from './ProjectForm.svelte';
import ScoreBar from './ScoreBar.svelte';
import StatusPill from './StatusPill.svelte';

const SET_URI = 'at://did:plc:neon/computer.localnetwork.dj.set/3kset';

const linkedMix = { title: 'Tidal Hours', url: 'https://mixes.test/tidal', set: { uri: SET_URI, cid: 'bafylinked' } };

const closing: DjSetView = {
	uri: SET_URI,
	cid: 'bafylinked',
	title: 'Closing',
	date: '2026-09-01',
	venue: 'Tresor',
	location: { country: 'DE', locality: 'Berlin' },
	trackCount: 24
};

/** A promise that never settles, for the moment before data arrives. */
const pending = <T>() => new Promise<T>(() => {});

describe('StatusPill', () => {
	it('humanizes statuses', async () => {
		await render(StatusPill, { status: 'changes_requested' });
		await expect.element(page.getByText('Changes requested')).toBeInTheDocument();
	});
});

describe('Avatar', () => {
	it('falls back to initials when there is no image', async () => {
		await render(Avatar, { name: 'Neon Tide', src: null });
		await expect.element(page.getByText('NT')).toBeInTheDocument();
	});
});

describe('ClaimFields', () => {
	it('asks for proof that names the claimant by handle', async () => {
		await render(ClaimFields, { values: {}, claimant: { did: 'did:plc:neon', handle: 'neon.test' } });
		await expect
			.element(page.getByRole('textbox', { name: 'How can a moderator verify this?' }))
			.toHaveAccessibleDescription(/localnetwork @neon\.test/);
	});

	it('names the claimant by DID when they have no handle', async () => {
		await render(ClaimFields, { values: {}, claimant: { did: 'did:plc:neon', handle: null } });
		await expect.element(page.getByText('localnetwork did:plc:neon')).toBeInTheDocument();
	});

	it('keeps what was entered, and starts over from new values', async () => {
		const claimant = { did: 'did:plc:pat', handle: 'pat.test' };

		const { rerender } = await render(ClaimFields, {
			values: { relationship: 'manager', links: 'https://a.test' },
			claimant
		});

		const links = page.getByRole('textbox', { name: 'Supporting links' });

		await expect.element(page.getByRole('radio', { name: "I'm their manager" })).toBeChecked();
		await page.getByRole('radio', { name: "I'm their booking agent" }).click();
		await links.fill('https://b.test');
		await expect.element(page.getByRole('radio', { name: "I'm their booking agent" })).toBeChecked();
		await expect.element(links).toHaveValue('https://b.test');

		await rerender({ values: { relationship: 'member', links: 'https://c.test' }, claimant });
		await expect.element(page.getByRole('radio', { name: "I'm a member of this group" })).toBeChecked();
		await expect.element(links).toHaveValue('https://c.test');
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

describe('PerformerCard', () => {
	it('links to the project and marks unclaimed projects', async () => {
		await render(PerformerCard, {
			performer: projectSummary({
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
			})
		});
		await expect.element(page.getByRole('link', { name: 'Neon Tide' })).toHaveAttribute('href', '/projects/neon-tide');
		await expect.element(page.getByText('Unclaimed')).toBeInTheDocument();
		await expect.element(page.getByText('3.2 km')).toBeInTheDocument();
	});

	it("links a listed DJ to their profile, by the genres they were found by, without a place they didn't give", async () => {
		await render(PerformerCard, {
			performer: djSummary({
				profile: {
					did: 'did:plc:neon',
					uri: 'at://did:plc:neon/computer.localnetwork.profile/self',
					cid: 'c',
					record: {
						basics: { name: 'Neon' },
						performer: { tagline: 'Hypnotic techno', fees: [{ tier: 'emerging' }] },
						createdAt: '2026-01-01T00:00:00Z',
						updatedAt: '2026-01-01T00:00:00Z'
					},
					avatarUrl: null
				},
				handle: 'neon.test',
				genres: ['Techno', 'Ambient']
			})
		});
		await expect.element(page.getByRole('link', { name: 'Neon' })).toHaveAttribute('href', '/@neon.test');
		await expect.element(page.getByText('Solo DJ', { exact: true })).toBeInTheDocument();
		await expect.element(page.getByText('Ambient')).toBeInTheDocument();
		await expect.element(page.getByText('Emerging')).toBeInTheDocument();
		await expect.element(page.getByText('Unclaimed')).not.toBeInTheDocument();
	});
});

describe('MixList', () => {
	it('renders mixes that repeat a link', async () => {
		const mix = { title: 'Tidal Hours', url: 'https://soundcloud.com/neon/tidal-hours' };
		await render(MixList, { mixes: [mix, { ...mix }] });
		await expect.element(page.getByRole('button', { name: 'Play Tidal Hours' }).nth(1)).toBeInTheDocument();
	});

	it('opens a player on demand and links a mix with a report to its tracklist', async () => {
		const tracklist = { $type: 'blob' as const, ref: { $link: 'bafkrei' }, mimeType: 'application/json', size: 1 };
		await render(MixList, {
			mixes: [{ title: 'Tidal Hours', url: 'https://soundcloud.com/neon/tidal-hours', durationMinutes: 90, tracklist }],
			tracklistHref: (i: number) => `/projects/neon-tide/mixes/${i + 1}`
		});
		await expect.element(page.getByText('SoundCloud · 1 h 30 min')).toBeInTheDocument();
		await expect
			.element(page.getByRole('link', { name: 'Tracklist' }))
			.toHaveAttribute('href', '/projects/neon-tide/mixes/1');
		// Clicking would load SoundCloud's player, so this stops at the closed state.
		await expect
			.element(page.getByRole('button', { name: 'Play Tidal Hours' }))
			.toHaveAttribute('aria-expanded', 'false');
		await expect.element(page.getByTitle('Tidal Hours')).not.toBeInTheDocument();
	});
});

describe('MixList linked sets', () => {
	const show = (sets: Promise<Map<string, DjSetView> | null>) => render(MixList, { mixes: [linkedMix], sets });

	it('shows the set by its at:// URI, with when and where it was played', async () => {
		await show(Promise.resolve(new Map([[SET_URI, closing]])));
		await expect.element(page.getByRole('link', { name: 'Closing' })).toHaveAttribute('href', SET_URI);
		await expect.element(page.getByText(/Sep 1, 2026 · Tresor, Berlin · 24 tracks/)).toBeInTheDocument();
		await expect.element(page.getByText('Changed since it was linked')).not.toBeInTheDocument();
	});

	it('says when the set changed since it was linked', async () => {
		await show(Promise.resolve(new Map([[SET_URI, { ...closing, cid: 'bafyedited' }]])));
		await expect.element(page.getByText(/Changed since it was linked/)).toBeInTheDocument();
	});

	it('holds a line for the set while it loads', async () => {
		await show(pending());
		await expect.element(page.getByText('Loading the set')).toBeInTheDocument();
	});

	it('says when the set is gone, and falls back to the link when the directory is down', async () => {
		await show(Promise.resolve(new Map()));
		await expect.element(page.getByText('Linked set no longer published')).toBeInTheDocument();

		await show(Promise.resolve(null));
		await expect.element(page.getByRole('link', { name: 'linked set' })).toHaveAttribute('href', SET_URI);
	});
});

describe('MixReport', () => {
	it('renders a report and shares the selected track between views', async () => {
		await render(MixReport, { report: sampleReport });
		await expect.element(page.getByRole('heading', { level: 1, name: 'Sunday Warm-up' })).toBeInTheDocument();

		for (const name of ['Timeline', 'Tracklist', 'Key moves', 'Tempo']) {
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

	it('posts what was entered, and starts over from new values', async () => {
		const { rerender } = await render(ProjectForm, {
			values: {
				name: 'Neon Tide',
				formats: ['vinyl'],
				fees: [{ label: 'Club nights', tier: '', currency: '', minAmount: '', maxAmount: '' }]
			},
			avatarUrl: '/avatar.png',
			geocoder: false
		});

		const posted = () => {
			const form = new FormData(document.querySelector('form')!);

			return {
				name: form.get('name'),
				formats: form.getAll('formats'),
				removeAvatar: form.get('removeAvatar'),
				fee: form.get('feeLabel')
			};
		};

		await page.getByRole('textbox', { name: 'Project name' }).fill('Neon Tide Live');
		await page.getByText('Modular').click();
		await page.getByRole('checkbox', { name: 'Remove the current image' }).click();
		await page.getByRole('textbox', { name: 'What this fee covers' }).fill('Warehouse nights');
		expect(posted()).toEqual({
			name: 'Neon Tide Live',
			formats: ['vinyl', 'modular'],
			removeAvatar: 'on',
			fee: 'Warehouse nights'
		});

		await rerender({
			values: { name: 'Halflight', formats: ['cdj'], removeAvatar: 'on' },
			avatarUrl: '/avatar.png',
			geocoder: false
		});
		await expect.element(page.getByRole('textbox', { name: 'Project name' })).toHaveValue('Halflight');
		await expect.element(page.getByRole('checkbox', { name: 'Remove the current image' })).toBeChecked();
		expect(posted()).toMatchObject({ name: 'Halflight', formats: ['cdj'], removeAvatar: 'on' });
	});
});

describe('ProjectForm set picker', () => {
	const picker = () => page.getByRole('combobox', { name: 'Set (optional):' });
	const options = () => [...document.querySelectorAll<HTMLOptionElement>('select[name=mixSet] option')];
	const posted = () => new FormData(document.querySelector('form')!).getAll('mixSet');

	it("keeps the linked set while sets load, then offers the managers' sets", async () => {
		let resolve: (sets: DjSetView[]) => void = () => {};

		const sets = new Promise<DjSetView[]>((done) => (resolve = done));

		await render(ProjectForm, { values: { mixes: [linkedMix] }, sets, geocoder: false });

		await expect.element(picker()).toBeInTheDocument();
		expect(options().map((option) => [option.text, option.disabled])).toEqual([
			['None', false],
			['Linked set', false],
			['Loading sets…', true]
		]);
		expect(posted()).toEqual([`${SET_URI} bafylinked`]);

		const other = { ...closing, uri: `${SET_URI}2`, cid: 'bafyother', title: 'Opening', venue: undefined };
		resolve([{ ...closing, cid: 'bafyedited' }, other]);
		await expect.element(page.getByRole('option', { name: 'Opening · Sep 1, 2026' })).toBeInTheDocument();
		expect(options().map((option) => option.text)).toEqual([
			'None',
			'Closing · Sep 1, 2026 (earlier version)',
			'Closing · Sep 1, 2026',
			'Opening · Sep 1, 2026'
		]);

		await picker().selectOptions('Opening · Sep 1, 2026');
		expect(posted()).toEqual([`${SET_URI}2 bafyother`]);
		await picker().selectOptions('None');
		expect(posted()).toEqual(['']);
	});

	it('says when there are no sets to pick, and where they come from', async () => {
		await render(ProjectForm, {
			values: { mixes: [{ title: 'New', url: '' }] },
			sets: Promise.resolve([]),
			geocoder: false
		});
		await expect.element(page.getByRole('option', { name: 'No published sets yet' })).toBeDisabled();
		await expect.element(page.getByRole('link', { name: 'locallibrary sets publish' })).toBeInTheDocument();
	});

	it("keeps the linked set when sets can't load", async () => {
		await render(ProjectForm, { values: { mixes: [linkedMix] }, sets: Promise.resolve(null), geocoder: false });
		await expect.element(page.getByRole('option', { name: "Couldn't load sets" })).toBeDisabled();
		expect(posted()).toEqual([`${SET_URI} bafylinked`]);
	});

	it('has no picker without sets to pick from', async () => {
		await render(ProjectForm, { values: { mixes: [linkedMix] }, geocoder: false });
		await expect.element(picker()).not.toBeInTheDocument();
		expect(posted()).toEqual([]);
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
