import { createRawSnippet } from 'svelte';
import { page, userEvent } from 'vitest/browser';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { ACCESS_PRESETS, epkFormDefaults } from '#lib/epk.ts';
import { stubFormAction } from '#lib/testing/form-action.ts';
import AccessMatrix from './AccessMatrix.svelte';
import EpkAccessCard from './EpkAccessCard.svelte';
import EpkForm from './EpkForm.svelte';
import EpkPhotos from './EpkPhotos.svelte';

afterEach(() => vi.unstubAllGlobals());

describe('AccessMatrix', () => {
	it('shows the preset as a rule per group, and becomes your own mix when one changes', async () => {
		await render(AccessMatrix, { access: ACCESS_PRESETS[1].access, preset: ACCESS_PRESETS[1].id });

		await expect.element(page.getByRole('radio', { name: /Promoters automatically/ })).toBeChecked();
		await expect.element(page.getByRole('radio', { name: 'Anyone signed in: You approve' })).toBeChecked();

		await userEvent.click(page.getByRole('radio', { name: 'Anyone signed in: Approved automatically' }));
		await expect.element(page.getByRole('radio', { name: /^Open/ })).toBeChecked();

		await userEvent.click(page.getByRole('radio', { name: "Promoters: Can't ask" }));
		await expect.element(page.getByRole('radio', { name: /Your own mix/ })).toBeChecked();

		await userEvent.click(page.getByRole('radio', { name: /Nobody can ask/ }));
		await expect.element(page.getByRole('radio', { name: "People you've booked with: Can't ask" })).toBeChecked();
	});

	it('keeps the rules on screen when the owner picks their own mix', async () => {
		await render(AccessMatrix, { access: ACCESS_PRESETS[1].access, preset: ACCESS_PRESETS[1].id });

		await userEvent.click(page.getByRole('radio', { name: /Your own mix/ }));

		await expect.element(page.getByRole('radio', { name: /Your own mix/ })).toBeChecked();
		await expect.element(page.getByRole('radio', { name: 'Promoters: Approved automatically' })).toBeChecked();
		await expect.element(page.getByRole('radio', { name: 'Anyone signed in: You approve' })).toBeChecked();
	});
});

describe('EpkAccessCard', () => {
	const props = { name: 'Neon Tide', askHref: '/dashboard/access/did:plc:dj/3k' };

	it('asks a visitor to sign in, saying when they get straight in', async () => {
		await render(EpkAccessCard, { ...props, access: { kind: 'signedOut', rule: 'auto' } });

		await expect.element(page.getByRole('link', { name: 'Sign in to see it' })).toHaveAttribute('href', props.askHref);
	});

	it('sends someone who can ask to the dashboard, where the form is', async () => {
		const { rerender } = await render(EpkAccessCard, { ...props, access: { kind: 'canAsk', rule: 'review' } });

		await expect.element(page.getByRole('link', { name: 'Ask for access' })).toHaveAttribute('href', props.askHref);
		await expect.element(page.getByRole('button')).not.toBeInTheDocument();
		await rerender({ ...props, access: { kind: 'canAsk', rule: 'auto' } });
		await expect.element(page.getByRole('link', { name: 'Get access' })).toHaveAttribute('href', props.askHref);
	});

	it('lets someone ask on the dashboard, and says the owner sees their handle', async () => {
		await render(EpkAccessCard, { name: 'Neon Tide', access: { kind: 'canAsk', rule: 'review' } });

		await expect.element(page.getByRole('button', { name: 'Ask for access' })).toBeVisible();
		await expect.element(page.getByLabelText("You're a")).toBeVisible();
		await expect.element(page.getByText(/Neon Tide decides\. Asking shares your handle/)).toBeVisible();
	});

	it('tells someone where their request stands', async () => {
		const pending = await render(EpkAccessCard, {
			...props,
			access: { kind: 'pending', since: '2026-10-13T12:00:00Z', approved: false }
		});

		await expect.element(page.getByText(/You asked on Oct 13, 2026/)).toBeVisible();
		await pending.rerender({ ...props, access: { kind: 'pending', since: '2026-10-13T12:00:00Z', approved: true } });
		await expect.element(page.getByText(/You're approved/)).toBeVisible();
		await pending.rerender({ ...props, access: { kind: 'closed' } });
		await expect.element(page.getByText(/isn't taking requests/)).toBeVisible();
		await expect.element(page.getByRole('button')).not.toBeInTheDocument();
	});

	it('asks a visitor who would have to wait to sign in to ask for it', async () => {
		await render(EpkAccessCard, { ...props, access: { kind: 'signedOut', rule: 'review' } });

		await expect
			.element(page.getByRole('link', { name: 'Sign in to ask for it' }))
			.toHaveAttribute('href', props.askHref);
	});

	it('lets someone who gets straight in open it from the dashboard', async () => {
		await render(EpkAccessCard, { name: 'Neon Tide', access: { kind: 'canAsk', rule: 'auto' } });

		await expect.element(page.getByRole('button', { name: 'See the press kit' })).toBeVisible();
		await expect.element(page.getByText(/You get in straight away\./)).toBeVisible();
	});

	it("shows why the last request didn't go through", async () => {
		await render(EpkAccessCard, {
			name: 'Neon Tide',
			access: { kind: 'canAsk', rule: 'review' },
			error: 'Pick what you are.'
		});

		await expect.element(page.getByRole('alert')).toHaveTextContent('Pick what you are.');
	});

	it('sends the role and message, and can be sent again once the answer is back', async () => {
		const action = stubFormAction();
		await render(EpkAccessCard, { name: 'Neon Tide', access: { kind: 'canAsk', rule: 'review' } });
		const ask = page.getByRole('button', { name: 'Ask for access' });

		await page.getByLabelText("You're a").selectOptions('Press');
		await page.getByLabelText(/Message/).fill('Writing up the Halle Nord weekend');
		await userEvent.click(ask);

		await expect.element(ask).toBeDisabled();
		await expect.poll(() => action.sent.length).toBe(1);
		expect(action.sent[0].get('role')).toBe('press');
		expect(action.sent[0].get('message')).toBe('Writing up the Halle Nord weekend');
		action.answer();
		await expect.element(ask).toBeEnabled();
	});

	it('tells someone who was turned down, and shows nothing to someone already in', async () => {
		const screen = await render(EpkAccessCard, { ...props, access: { kind: 'declined' } });

		await expect.element(page.getByText("Neon Tide isn't sharing the rest of this press kit with you.")).toBeVisible();
		await screen.rerender({ ...props, access: { kind: 'granted', since: '2026-10-13T12:00:00Z' } });
		await expect.element(page.getByText(/isn't sharing/)).not.toBeInTheDocument();
		await expect.element(page.getByRole('link')).not.toBeInTheDocument();
		await expect.element(page.getByRole('status')).not.toBeInTheDocument();
	});
});

describe('EpkPhotos', () => {
	it('links each photo and its download, marking the ones only people with access see', async () => {
		const image = (cid: string) => ({ $type: 'blob' as const, ref: { $link: cid }, mimeType: 'image/png', size: 1 });

		await render(EpkPhotos, {
			base: '/epk/did:plc:dj/3k/photos',
			photos: [
				{ image: image('bafyone'), alt: 'Neon Tide at Halle Nord', credit: 'Mara Okafor' },
				{
					image: image('bafytwo'),
					alt: 'Neon Tide behind the decks',
					aspectRatio: { width: 1600, height: 1600 },
					onRequest: true
				}
			]
		});

		await expect
			.element(page.getByRole('img', { name: 'Neon Tide at Halle Nord' }))
			.toHaveAttribute('src', '/epk/did:plc:dj/3k/photos/bafyone');
		await expect
			.element(page.getByRole('link', { name: 'Download photo by Mara Okafor' }))
			.toHaveAttribute('href', '/epk/did:plc:dj/3k/photos/bafyone?download');
		await expect.element(page.getByLabelText('On request')).toBeInTheDocument();
		await expect.element(page.getByText('1600 × 1600')).toBeInTheDocument();
	});

	it('leaves the caption empty for a photo with no credit or size', async () => {
		await render(EpkPhotos, {
			base: '/epk/did:plc:dj/3k/photos',
			photos: [
				{ image: { $type: 'blob', ref: { $link: 'bafyone' }, mimeType: 'image/png', size: 1 }, alt: 'Neon Tide' }
			]
		});

		await expect.element(page.getByRole('link', { name: 'Download photo', exact: true })).toBeVisible();
		await expect.element(page.getByText(/Photo:| × /)).not.toBeInTheDocument();
	});
});

describe('EpkForm', () => {
	it('keeps what was entered in the rows it adds, and starts over from new values', async () => {
		const props = { photoBase: null, editHref: '/dashboard/projects/neon-tide/profile', submitLabel: 'Save changes' };
		const { rerender } = await render(EpkForm, { ...props, values: epkFormDefaults });
		const extraMix = page.getByRole('textbox', { name: 'Extra mix 2 name' });
		const quote = page.getByRole('textbox', { name: 'Quote 5', exact: true });

		await extraMix.fill('Promo');
		await page
			.getByRole('group', { name: 'Who can play extra mix 2' })
			.getByRole('radio', { name: 'Everyone' })
			.click();
		await quote.fill('Relentless');
		await expect.element(extraMix).toHaveValue('Promo');
		await expect
			.element(page.getByRole('group', { name: 'Who can play extra mix 2' }).getByRole('radio', { name: 'Everyone' }))
			.toBeChecked();
		await expect.element(quote).toHaveValue('Relentless');

		await rerender({ ...props, values: { ...epkFormDefaults, quotes: [{ text: 'Patient', source: 'Mixmag' }] } });
		await expect.element(extraMix).toHaveValue('');
		await expect
			.element(page.getByRole('group', { name: 'Who can play extra mix 2' }).getByRole('radio', { name: 'On request' }))
			.toBeChecked();
		await expect.element(quote).toHaveValue('');
		await expect.element(page.getByRole('textbox', { name: 'Quote 1', exact: true })).toHaveValue('Patient');
	});

	it('shows saved photos with their size, and the press checklist they pass', async () => {
		await render(EpkForm, {
			values: {
				...epkFormDefaults,
				title: 'Clubs',
				photos: [
					{ cid: 'bafyone', alt: 'Wide', credit: 'Mara', visibility: 'public', width: 3200, height: 1800 },
					{ cid: 'bafytwo', alt: 'Tall', credit: '', visibility: 'private', width: 1000, height: 1500 }
				],
				extraMixes: [{ title: 'Promo', url: 'https://soundcloud.com/neon/promo', visibility: 'private' }]
			},
			photoBase: '/epk/did:plc:dj/3k/photos',
			editHref: '/dashboard/projects/neon-tide/profile',
			submitLabel: 'Save changes'
		});

		await expect
			.element(page.getByRole('img', { name: 'Wide' }))
			.toHaveAttribute('src', '/epk/did:plc:dj/3k/photos/bafyone');
		await expect.element(page.getByText('Press ready')).toBeVisible();
		await expect.element(page.getByText('Web size')).toBeVisible();
		await expect
			.element(page.getByRole('group', { name: 'Who sees photo 2' }).getByRole('radio', { name: 'On request' }))
			.toBeChecked();
		await expect.element(page.getByRole('textbox', { name: 'Extra mix 1 name' })).toHaveValue('Promo');
		await expect.element(page.getByRole('radio', { name: 'On request' }).nth(2)).toBeChecked();
		await expect
			.element(page.getByRole('link', { name: 'Add some to the project' }))
			.toHaveAttribute('href', '/dashboard/projects/neon-tide/profile');
		await expect
			.element(page.getByRole('listitem').filter({ hasText: 'Every photo credited' }))
			.not.toHaveClass(/text-good/);
	});

	it("lists the project's mixes, each with who can play it", async () => {
		await render(EpkForm, {
			values: {
				...epkFormDefaults,
				mixes: [{ title: 'Halle Nord closing', url: 'https://soundcloud.com/neon/halle-nord', visibility: 'public' }]
			},
			photoBase: null,
			editHref: '/dashboard/projects/neon-tide/profile',
			submitLabel: 'Save changes'
		});
		const mix = page.getByRole('group', { name: 'Who can play Halle Nord closing' });

		await expect.element(mix.getByRole('radio', { name: 'Everyone' })).toBeChecked();
		await userEvent.click(mix.getByRole('radio', { name: 'On request' }));

		await expect.element(mix.getByRole('radio', { name: 'On request' })).toBeChecked();
		await expect.element(page.getByRole('link', { name: 'Add some to the project' })).not.toBeInTheDocument();
	});

	it('ticks off each press check a set of photos passes', async () => {
		const photo = (cid: string, width: number, height: number) => ({
			cid,
			alt: `Neon Tide, ${width} by ${height}`,
			credit: 'Mara Okafor',
			visibility: 'public' as const,
			width,
			height
		});

		await render(EpkForm, {
			values: {
				...epkFormDefaults,
				photos: [photo('bafywide', 4500, 3000), photo('bafytall', 3000, 4500), photo('bafysquare', 3000, 3000)]
			},
			photoBase: null,
			editHref: '/dashboard/projects/neon-tide/profile',
			submitLabel: 'Save changes'
		});
		const checks = page.getByRole('list', { name: 'Press photo checklist' }).getByRole('listitem');

		await expect.element(checks).toHaveLength(4);

		for (const check of checks.all()) await expect.element(check).toHaveClass(/text-good/);
		await expect.element(page.getByRole('img')).not.toBeInTheDocument();
	});

	it('shows no size for a photo saved without one, and can remove it', async () => {
		await render(EpkForm, {
			values: { ...epkFormDefaults, photos: [{ cid: 'bafyold', alt: 'Old flyer', credit: '', visibility: 'public' }] },
			photoBase: '/epk/did:plc:dj/3k/photos',
			editHref: '/dashboard/projects/neon-tide/profile',
			submitLabel: 'Save changes'
		});
		const photo = page.getByRole('group', { name: 'Who sees photo 1' });

		await expect.element(page.getByText(/ × /)).not.toBeInTheDocument();
		await userEvent.click(photo.getByRole('radio', { name: 'Remove' }));
		await expect.element(photo.getByRole('radio', { name: 'Remove' })).toBeChecked();
	});

	it("sends what's entered, holding the button while it saves", async () => {
		const action = stubFormAction();
		await render(EpkForm, {
			values: epkFormDefaults,
			action: '?/publish',
			photoBase: null,
			editHref: '/dashboard/projects/neon-tide/profile',
			submitLabel: 'Publish',
			actions: createRawSnippet(() => ({ render: () => '<a href="/dashboard/projects/neon-tide">Cancel</a>' }))
		});

		await page.getByRole('textbox', { name: 'Name' }).fill('Clubs');
		await page.getByRole('textbox', { name: 'Short bio' }).fill('Berlin DJ duo.');
		await page.getByRole('textbox', { name: 'Full bio' }).fill('Neon Tide plays long, patient sets.');
		await page.getByRole('textbox', { name: 'Extra mix 1 address' }).fill('https://soundcloud.com/neon/promo');
		await page.getByRole('textbox', { name: 'Who said quote 1' }).fill('Mixmag');
		await page.getByRole('textbox', { name: 'Link for quote 1' }).fill('https://mixmag.net/neon-tide');
		await page.getByRole('textbox', { name: 'Tech rider' }).fill('2x CDJ-3000, DJM-V10');
		await expect.element(page.getByRole('link', { name: 'Cancel' })).toBeVisible();
		await userEvent.click(page.getByRole('button', { name: 'Publish' }));

		await expect.element(page.getByRole('button', { name: 'Saving…' })).toBeDisabled();
		await expect.poll(() => action.sent.length).toBe(1);
		const sent = action.sent[0];
		expect(sent.get('title')).toBe('Clubs');
		expect(sent.get('bio')).toBe('Berlin DJ duo.');
		expect(sent.get('longBio')).toBe('Neon Tide plays long, patient sets.');
		expect(sent.getAll('extraMixUrl')).toEqual(['https://soundcloud.com/neon/promo', '']);
		expect(sent.getAll('quoteSource')[0]).toBe('Mixmag');
		expect(sent.getAll('quoteUrl')[0]).toBe('https://mixmag.net/neon-tide');
		expect(sent.get('techRider')).toBe('2x CDJ-3000, DJM-V10');
		action.answer();
		await expect.element(page.getByRole('button', { name: 'Publish' })).toBeEnabled();
		await expect.element(page.getByRole('textbox', { name: 'Name' })).toHaveValue('Clubs');
	});
});
