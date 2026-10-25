import { page, userEvent } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { ACCESS_PRESETS, epkFormDefaults } from '#lib/epk.ts';
import AccessMatrix from './AccessMatrix.svelte';
import EpkAccessCard from './EpkAccessCard.svelte';
import EpkForm from './EpkForm.svelte';
import EpkPhotos from './EpkPhotos.svelte';

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
});

describe('EpkForm', () => {
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
});
