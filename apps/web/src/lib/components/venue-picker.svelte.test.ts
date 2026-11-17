import { page, userEvent } from 'vitest/browser';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { Place } from '#lib/places.ts';
import type { AddVenueResponse, VenueOption } from '#lib/venues.ts';
import VenuePicker from './VenuePicker.svelte';

const tresor: VenueOption = {
	space: 'at://did:plc:root/computer.localnetwork.space.venue/3tresor',
	skey: '3tresor',
	name: 'Tresor',
	locality: 'Berlin',
	country: 'DE'
};

const berlin: Place = { label: 'Berlin, Germany', city: 'Berlin', country: 'DE', latE6: 52_520_000, lngE6: 13_405_000 };

/**
 * Stands in for `/api/venues` and `/api/places`: venues and places by query, and `added` for an add. Returns what was
 * asked, in order, with each add's body.
 */
function serve(options: { venues?: Record<string, VenueOption[]>; added?: AddVenueResponse; status?: number }) {
	const asked: string[] = [];

	vi.stubGlobal('fetch', async (url: string, init?: RequestInit) => {
		const { pathname, searchParams } = new URL(url, location.href);
		const q = searchParams.get('q') ?? '';

		if (init?.method === 'POST') {
			asked.push(`POST ${pathname} ${await new Request(url, init).text()}`);

			return Response.json(options.added, { status: options.status ?? 200 });
		}

		asked.push(url);

		return pathname === '/api/places'
			? Response.json({ places: q === 'Berl' ? [berlin] : [] })
			: Response.json({ venues: options.venues?.[q] ?? [] });
	});

	return asked;
}

const search = () => page.getByRole('combobox', { name: 'Venue' });

const posted = () => document.querySelector<HTMLInputElement>('input[name=venue]')?.value;

afterEach(() => vi.unstubAllGlobals());

describe('VenuePicker', () => {
	it('picks an existing venue by name, told apart from others of that name by its city, and posts its space', async () => {
		const hamburg: VenueOption = { ...tresor, space: `${tresor.space}-hh`, skey: '3tresorhh', locality: 'Hamburg' };
		const asked = serve({ venues: { Tres: [tresor, hamburg] } });
		await render(VenuePicker);

		await search().fill('Tres');
		await expect.element(page.getByRole('option', { name: 'Tresor Hamburg, DE' })).toBeVisible();
		await page.getByRole('option', { name: 'Tresor Berlin, DE' }).click();

		expect(asked).toEqual(['/api/venues?q=Tres']);
		await expect.element(page.getByText('Tresor')).toBeVisible();
		await expect.element(page.getByText('Berlin, DE')).toBeVisible();
		expect(posted()).toBe(tresor.space);

		await page.getByRole('button', { name: 'Change venue' }).click();
		await expect.element(search()).toHaveValue('');
		expect(posted()).toBeUndefined();
	});

	it("picks a venue and a new venue's city from the keyboard", async () => {
		serve({ venues: { Tres: [tresor] } });
		await render(VenuePicker);

		await search().fill('Tres');
		await expect.element(page.getByRole('option', { name: 'Tresor Berlin, DE' })).toBeVisible();
		await userEvent.keyboard('{ArrowDown}{Enter}');
		expect(posted()).toBe(tresor.space);

		await page.getByRole('button', { name: 'Change venue' }).click();
		await page.getByRole('button', { name: 'Add a new venue' }).click();
		await page.getByRole('combobox', { name: 'City' }).fill('Berl');
		await expect.element(page.getByRole('option', { name: 'Berlin, Germany' })).toBeVisible();
		await userEvent.keyboard('{ArrowDown}{Enter}');
		await expect.element(page.getByRole('combobox', { name: 'City' })).toHaveValue('Berlin, Germany');
	});

	it('says when no venue matches, and offers to add it', async () => {
		serve({});
		await render(VenuePicker);

		await search().fill('Kater Blau');

		await expect.element(page.getByText('No venues match "Kater Blau".')).toBeVisible();
		await page.getByRole('button', { name: 'Add "Kater Blau" as a new venue' }).click();
		await expect.element(page.getByRole('textbox', { name: 'Name' })).toHaveValue('Kater Blau');
	});

	it('adds a new venue with its city from the search', async () => {
		const added: VenueOption = {
			...tresor,
			space: 'at://did:plc:root/computer.localnetwork.space.venue/3kater',
			skey: '3kater',
			name: 'Kater Blau'
		};

		const asked = serve({ added: { venue: added } });
		await render(VenuePicker);

		await page.getByRole('button', { name: 'Add a new venue' }).click();
		await page.getByRole('textbox', { name: 'Name' }).fill('Kater Blau');

		// Without a city from the search, it asks for one.
		await page.getByRole('button', { name: 'Add venue' }).click();
		await expect.element(page.getByRole('alert')).toHaveTextContent('Pick the city from the search');

		await page.getByRole('combobox', { name: 'City' }).fill('Berl');
		await page.getByRole('option', { name: 'Berlin, Germany' }).click();
		await page.getByRole('button', { name: 'Add venue' }).click();

		await expect.element(page.getByText('Kater Blau')).toBeVisible();
		expect(posted()).toBe(added.space);
		expect(asked.at(-1)).toBe(`POST /api/venues ${JSON.stringify({ name: 'Kater Blau', city: berlin })}`);
	});

	it('shows why an add was refused, and keeps the form', async () => {
		serve({ added: { error: 'Sign in to add a venue' }, status: 401 });
		await render(VenuePicker);

		await page.getByRole('button', { name: 'Add a new venue' }).click();
		await page.getByRole('textbox', { name: 'Name' }).fill('Kater Blau');
		await page.getByRole('combobox', { name: 'City' }).fill('Berl');
		await page.getByRole('option', { name: 'Berlin, Germany' }).click();
		await page.getByRole('button', { name: 'Add venue' }).click();

		await expect.element(page.getByRole('alert')).toHaveTextContent('Sign in to add a venue');
		expect(posted()).toBeUndefined();

		await page.getByRole('button', { name: 'Cancel' }).click();
		await expect.element(search()).toBeVisible();
	});

	it('says when an add could not reach the site', async () => {
		serve({});
		vi.stubGlobal('fetch', async (url: string, init?: RequestInit) => {
			if (init?.method === 'POST') throw new TypeError('Failed to fetch');

			return Response.json({ places: [berlin] });
		});
		await render(VenuePicker);

		await page.getByRole('button', { name: 'Add a new venue' }).click();
		await page.getByRole('textbox', { name: 'Name' }).fill('Kater Blau');
		await page.getByRole('combobox', { name: 'City' }).fill('Berl');
		await page.getByRole('option', { name: 'Berlin, Germany' }).click();
		await page.getByRole('button', { name: 'Add venue' }).click();

		await expect
			.element(page.getByRole('alert'))
			.toHaveTextContent("Couldn't add the venue. Check your connection and try again.");
		await expect.element(page.getByRole('button', { name: 'Add venue' })).toBeEnabled();
	});

	it('starts on the venue it was given', async () => {
		serve({});
		await render(VenuePicker, { value: tresor, name: 'where' });

		await expect.element(page.getByText('Berlin, DE')).toBeVisible();
		expect(document.querySelector<HTMLInputElement>('input[name=where]')?.value).toBe(tresor.space);
	});
});
