import { tick } from 'svelte';
import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { posted, renderInForm } from '#lib/testing/forms.ts';
import type { DjSetView } from '#lib/types.ts';
import BasicsFields from './form/BasicsFields.svelte';
import PerformerFields from './form/PerformerFields.svelte';
import PromoterFields from './form/PromoterFields.svelte';

const SET_URI = 'at://did:plc:neon/computer.localnetwork.dj.set/3kset';

const closing: DjSetView = { uri: SET_URI, cid: 'bafyclosing', title: 'Closing', date: '2026-09-01', trackCount: 24 };

const tidalHours = { title: 'Tidal Hours', url: 'https://mixes.test/tidal' };

const picker = () => page.getByRole('combobox', { name: 'Set (optional):' });

const options = () =>
	[...document.querySelectorAll<HTMLOptionElement>('select[name=mixSet] option')].map((option) => option.text);

describe('BasicsFields', () => {
	it('posts what was entered', async () => {
		const { form } = await renderInForm(BasicsFields, {
			description: 'Who you are.',
			avatarUrl: '/avatar.png',
			geocoder: false
		});

		await page.getByLabelText('Name', { exact: true }).fill('Neon Tide');
		await page.getByLabelText('Pronouns (optional)').fill('they/them');
		await page.getByLabelText('Bio').fill('Rolling techno from Leipzig.');
		await page.getByRole('combobox', { name: 'Link type' }).selectOptions('Bandcamp');
		await page.getByRole('textbox', { name: 'Link URL' }).fill('https://neon.bandcamp.test');
		await page.getByRole('checkbox', { name: 'Remove the current image' }).click();
		await page.getByRole('checkbox', { name: 'Remove the current image' }).click();

		expect(posted(form, 'name', 'pronouns', 'bio', 'linkKind', 'linkUrl', 'removeAvatar')).toEqual({
			name: ['Neon Tide'],
			pronouns: ['they/them'],
			bio: ['Rolling techno from Leipzig.'],
			linkKind: ['bandcamp'],
			linkUrl: ['https://neon.bandcamp.test'],
			removeAvatar: []
		});
	});

	it("shows a person's pronouns from Bluesky without a field, and posts none", async () => {
		const { form } = await renderInForm(BasicsFields, {
			description: 'Who you are.',
			geocoder: false,
			pronouns: 'she/her',
			values: { pronouns: 'they/them' }
		});

		await expect.element(page.getByText('she/her')).toBeInTheDocument();
		await expect.element(page.getByText('Change them in your Bluesky profile.')).toBeInTheDocument();
		await expect.element(page.getByLabelText(/Pronouns/)).not.toBeInTheDocument();
		expect(posted(form, 'pronouns')).toEqual({ pronouns: [] });
	});

	it('adds links up to twelve', async () => {
		const { form } = await renderInForm(BasicsFields, { description: 'Who you are.', geocoder: false });

		for (let added = 1; added < 12; added++) await page.getByRole('button', { name: 'Add link' }).click();

		expect(page.getByRole('textbox', { name: 'Link URL' }).elements()).toHaveLength(12);
		await expect.element(page.getByRole('button', { name: 'Add link' })).not.toBeInTheDocument();
		expect(posted(form, 'linkKind').linkKind).toEqual(['soundcloud', ...Array(11).fill('website')]);
	});
});

describe('PerformerFields', () => {
	it('starts with one empty fee and one empty mix', async () => {
		await renderInForm(PerformerFields, {});

		expect(page.getByRole('textbox', { name: 'What this fee covers' }).elements()).toHaveLength(1);
		expect(page.getByRole('textbox', { name: 'Mix title' }).elements()).toHaveLength(1);
		await expect.element(page.getByLabelText('Tracklist for this mix')).toBeInTheDocument();
		await expect.element(picker()).not.toBeInTheDocument();
	});

	it('posts what was entered', async () => {
		const { form } = await renderInForm(PerformerFields, {});

		await page.getByLabelText('Tagline').fill('Hypnotic, rolling techno');
		await page.getByLabelText('Will travel up to (km)').fill('300');
		await page.getByRole('checkbox', { name: 'Open to international bookings' }).click();
		await page.getByLabelText('Travel notes').fill('Needs a hotel');
		await page.getByText('Peak time').click();
		await page.getByText('Vinyl').click();
		await page.getByLabelText('Shortest set (min)').fill('60');
		await page.getByLabelText('Longest set (min)').fill('240');
		await page.getByLabelText('Smallest room').fill('100');
		await page.getByLabelText('Biggest room').fill('800');
		await page.getByLabelText('Playing since').fill('2016');
		await page.getByRole('combobox', { name: 'Fee tier' }).selectOptions('Emerging');
		await page.getByRole('textbox', { name: 'What this fee covers' }).fill('Club nights');
		await page.getByRole('textbox', { name: 'Currency' }).fill('EUR');
		await page.getByRole('spinbutton', { name: 'From' }).fill('400');
		await page.getByRole('spinbutton', { name: 'Up to' }).fill('900');
		await page.getByRole('textbox', { name: 'Mix title' }).fill('Tidal Hours');
		await page.getByRole('textbox', { name: 'Mix URL' }).fill('https://mixes.test/tidal');
		await page.getByLabelText('Highlights').fill('Resident at Distillery');
		await page.getByLabelText('Looking for').fill('Warm-up slots');

		expect(
			posted(
				form,
				'tagline',
				'travelRadiusKm',
				'international',
				'travelNotes',
				'setTypes',
				'formats',
				'setMin',
				'setMax',
				'capacityMin',
				'capacityMax',
				'activeSince',
				'feeTier',
				'feeLabel',
				'feeCurrency',
				'feeMin',
				'feeMax',
				'mixTitle',
				'mixUrl',
				'highlights',
				'lookingFor'
			)
		).toEqual({
			tagline: ['Hypnotic, rolling techno'],
			travelRadiusKm: ['300'],
			international: ['on'],
			travelNotes: ['Needs a hotel'],
			setTypes: ['peak'],
			formats: ['vinyl'],
			setMin: ['60'],
			setMax: ['240'],
			capacityMin: ['100'],
			capacityMax: ['800'],
			activeSince: ['2016'],
			feeTier: ['emerging'],
			feeLabel: ['Club nights'],
			feeCurrency: ['EUR'],
			feeMin: ['400'],
			feeMax: ['900'],
			mixTitle: ['Tidal Hours'],
			mixUrl: ['https://mixes.test/tidal'],
			highlights: ['Resident at Distillery'],
			lookingFor: ['Warm-up slots']
		});
	});

	it('keeps each fee with its own tier as fees come and go', async () => {
		const { form } = await renderInForm(PerformerFields, {
			values: { fees: [{ label: 'Club nights', tier: 'emerging', currency: '', minAmount: '', maxAmount: '' }] }
		});

		await page.getByRole('button', { name: 'Add fee' }).click();
		await page.getByRole('textbox', { name: 'What this fee covers' }).last().fill('Festivals');
		await page.getByRole('combobox', { name: 'Fee tier' }).last().selectOptions('Headliner');
		await page.getByRole('button', { name: 'Remove fee' }).first().click();

		expect(posted(form, 'feeLabel', 'feeTier')).toEqual({ feeLabel: ['Festivals'], feeTier: ['headliner'] });
	});

	it('adds mixes up to twelve, and drops one', async () => {
		const { form } = await renderInForm(PerformerFields, { values: { mixes: [tidalHours] } });

		for (let added = 1; added < 12; added++) await page.getByRole('button', { name: 'Add mix' }).click();
		await expect.element(page.getByRole('button', { name: 'Add mix' })).not.toBeInTheDocument();
		await page.getByRole('button', { name: 'Remove mix' }).first().click();

		expect(posted(form, 'mixTitle').mixTitle).toEqual(Array(11).fill(''));
		await expect.element(page.getByRole('button', { name: 'Add mix' })).toBeVisible();
	});

	it('keeps an attached tracklist until it is removed', async () => {
		const tracklist = {
			$type: 'blob' as const,
			ref: { $link: 'bafytracks' },
			mimeType: 'application/json',
			size: 2048
		};

		const { form } = await renderInForm(PerformerFields, { values: { mixes: [{ ...tidalHours, tracklist }] } });

		await expect.element(page.getByText('Tracklist attached')).toBeVisible();
		expect(posted(form, 'mixTracklistCid')).toEqual({ mixTracklistCid: ['bafytracks'] });
		await page.getByRole('button', { name: 'Remove', exact: true }).click();

		await expect.element(page.getByText('Tracklist (optional):')).toBeVisible();
		await expect.element(page.getByLabelText('Tracklist for Tidal Hours')).toBeInTheDocument();
		expect(posted(form, 'mixTracklistCid')).toEqual({ mixTracklistCid: [''] });
	});

	it("says when the sets couldn't be asked for", async () => {
		await renderInForm(PerformerFields, {
			values: { mixes: [tidalHours] },
			sets: Promise.reject(new Error('PDS down'))
		});

		await expect.element(page.getByRole('option', { name: "Couldn't load sets" })).toBeDisabled();
	});

	it('marks a linked set that is no longer published', async () => {
		const gone = { uri: `${SET_URI}gone`, cid: 'bafygone' };

		const { form } = await renderInForm(PerformerFields, {
			values: { mixes: [{ ...tidalHours, set: gone }] },
			sets: Promise.resolve([closing])
		});

		await expect.element(page.getByRole('option', { name: 'Linked set (no longer published)' })).toBeInTheDocument();
		expect(options()).toEqual(['None', 'Linked set (no longer published)', 'Closing · Sep 1, 2026']);
		expect(posted(form, 'mixSet')).toEqual({ mixSet: [`${SET_URI}gone bafygone`] });
	});

	it('shows the sets it was last given, not ones asked for before', async () => {
		let answerEarlier: (sets: DjSetView[] | null) => void = () => {};

		const earlier = new Promise<DjSetView[] | null>((resolve) => (answerEarlier = resolve));
		const { rerender } = await render(PerformerFields, { values: { mixes: [tidalHours] }, sets: earlier });

		await rerender({ values: { mixes: [tidalHours] }, sets: Promise.resolve([closing]) });
		await expect.element(page.getByRole('option', { name: 'Closing · Sep 1, 2026' })).toBeInTheDocument();
		answerEarlier(null);
		// A macrotask, so every microtask the earlier answer queued has run, then the redraw.
		await new Promise((resolve) => setTimeout(resolve));
		await tick();

		expect(options()).toEqual(['None', 'Closing · Sep 1, 2026']);
	});
});

describe('PromoterFields', () => {
	it('starts with one empty city and posts what was entered', async () => {
		const { form } = await renderInForm(PromoterFields, {});

		await page.getByRole('textbox', { name: 'City' }).fill('Leipzig');
		await page.getByRole('textbox', { name: 'Region' }).fill('Saxony');
		await page.getByRole('textbox', { name: 'Country' }).fill('DE');

		expect(posted(form, 'bookCity', 'bookRegion', 'bookCountry')).toEqual({
			bookCity: ['Leipzig'],
			bookRegion: ['Saxony'],
			bookCountry: ['DE']
		});
	});

	it('adds cities up to twelve, and drops one', async () => {
		const { form } = await renderInForm(PromoterFields, {
			values: { cities: [{ city: 'Berlin', region: '', country: 'DE' }] }
		});

		for (let added = 1; added < 12; added++) await page.getByRole('button', { name: 'Add city' }).click();
		await expect.element(page.getByRole('button', { name: 'Add city' })).not.toBeInTheDocument();
		await page.getByRole('button', { name: 'Remove city' }).first().click();

		expect(posted(form, 'bookCity').bookCity).toEqual(Array(11).fill(''));
		await expect.element(page.getByRole('button', { name: 'Add city' })).toBeVisible();
	});
});
