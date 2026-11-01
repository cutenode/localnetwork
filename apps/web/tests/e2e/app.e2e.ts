import { expect, type Page, type Route, test } from '@playwright/test';
import headline from '../../src/lib/headline.json' with { type: 'json' };
import { z } from 'zod';
import { signIn } from '../sign-in.ts';
import { expectAccessible } from './axe.ts';

/**
 * WebKit on Linux won't keep a `Secure` cookie from plain-HTTP localhost, where the other browsers
 * (and WebKit on macOS) will, and the local PDS's sign-in form reads its CSRF token from one. So in
 * WebKit each Secure cookie the PDS sets is stored again without the flag, which protects nothing
 * on loopback anyway.
 */
function keepPdsCookies(page: Page) {
	if (page.context().browser()?.browserType().name() !== 'webkit') return;

	page.on('response', async (response) => {
		if (!response.url().startsWith('http://localhost:2683/')) return;

		for (const line of (await response.headerValue('set-cookie'))?.split('\n') ?? []) {
			const [pair, ...attributes] = line.split(';');

			if (!attributes.some((attribute) => attribute.trim().toLowerCase() === 'secure')) continue;

			const separator = pair.indexOf('=');
			const cookie = { name: pair.slice(0, separator).trim(), value: pair.slice(separator + 1).trim() };
			await page.context().addCookies([{ ...cookie, url: 'http://localhost:2683' }]);
		}
	});
}

test.beforeEach(({ page }) => keepPdsCookies(page));

/**
 * Signs out from the dashboard and waits for the redirect home: /logout records the audit entry and
 * revokes the OAuth session first, and a sign-in started before it finishes would find the old session.
 */
async function signOut(page: Page) {
	await page.goto('/dashboard');
	await page.getByRole('button', { name: 'Sign out' }).click();
	await page.waitForURL((url) => url.pathname === '/');
}

/**
 * Opens `path` with its scripts held back and runs `act` on the server-rendered page, as someone on a slow
 * connection can, then lets the page hydrate.
 */
async function beforeHydration(page: Page, path: string, act: () => Promise<void>) {
	const { promise: released, resolve: release } = Promise.withResolvers<void>();

	const holdScripts = async (route: Route) => {
		if (route.request().resourceType() === 'script') await released;
		await route.fallback();
	};

	await page.route('**/*', holdScripts);
	await page.goto(path);
	await act();
	release();
	await expect(page.locator('html')).toHaveAttribute('data-hydrated', '');
	await page.unroute('**/*', holdScripts);
}

test.describe('public pages', () => {
	test('home page lists seeded projects and links to search', async ({ page }) => {
		await page.goto('/');
		await expect(page.getByRole('heading', { level: 1 })).toHaveAccessibleName(
			`${headline.verbs[0]} your ${headline.nouns[0]}.`
		);
		await expect(page.getByRole('heading', { name: 'New on the directory' })).toBeVisible();
		await expectAccessible(page);
		// Narrower than the desktop layout, the header's links fold into a menu, so open it the way a
		// person on a phone would. Its button only responds once the page has hydrated.
		const menu = page.getByRole('button', { name: 'Menu' });

		if (await menu.isVisible()) {
			await expect(page.locator('html')).toHaveAttribute('data-hydrated', '');
			await menu.click();
		}

		await page.getByRole('link', { name: 'Find DJs' }).first().click();
		await expect(page).toHaveURL(/\/projects$/);
		await expect(page.getByRole('heading', { name: 'Find DJs' })).toBeVisible();
		await expectAccessible(page);
	});

	test('directory filters by genre family and opens a profile', async ({ page }) => {
		await page.goto('/projects?genres=house');
		await expect(page.getByRole('link', { name: 'Halflight' })).toBeVisible();
		await expect(page.getByRole('link', { name: 'Coyote Cumbia Club' })).toHaveCount(0);
		await expectAccessible(page);
		await page.getByRole('link', { name: 'Halflight' }).click();
		await expect(page.getByRole('heading', { name: 'Halflight' })).toBeVisible();
		await expect(page.getByText('Contacts, rates and the tech rider come with a press kit.')).toBeVisible();
		await expect(page.getByRole('link', { name: 'See the press kits' })).toHaveAttribute(
			'href',
			'/projects/halflight/press-kits'
		);
		await expectAccessible(page);
	});

	test('directory filters opened before the page hydrates stay open', async ({ page }) => {
		await beforeHydration(page, '/projects', async () => {
			// A phone folds the whole panel away first.
			const panel = page.locator('summary', { hasText: /^\s*Filters/ });

			if (await panel.isVisible()) await panel.click();
			await page.locator('summary', { hasText: 'More filters' }).click();
		});
		await expect(page.getByRole('group', { name: 'Format' })).toBeVisible();
	});

	test('directory filters set before the page hydrates stay set', async ({ page }) => {
		const search = page.getByLabel('Search');
		const chip = page.getByRole('group', { name: 'Fee tier' }).getByRole('checkbox').first();
		const sort = page.getByLabel('Sort by');

		await beforeHydration(page, '/projects', async () => {
			const panel = page.locator('summary', { hasText: /^\s*Filters/ });

			if (await panel.isVisible()) await panel.click();
			await search.fill('techno');
			await chip.locator('xpath=..').click();
			await sort.selectOption('name');
		});
		await expect(search).toHaveValue('techno');
		await expect(chip).toBeChecked();
		await expect(sort).toHaveValue('name');

		// Kept filters apply, and the form follows the page when they're cleared.
		await page.getByRole('button', { name: 'Apply' }).click();
		await expect(page).toHaveURL(/[?&]q=techno(&|$)/);
		await page.getByRole('link', { name: 'Clear filters' }).first().click();
		await expect(page).toHaveURL(/\/projects$/);
		await expect(search).toHaveValue('');
		await expect(chip).not.toBeChecked();
	});

	test('a profile shows how the DJ plays, and its press kit prints', async ({ page }) => {
		await page.goto('/projects/neon-tide');
		await expect(page.getByRole('heading', { name: 'How they play' })).toBeVisible();
		await expect(page.getByText('126–134')).toBeVisible();
		await expect(page.getByRole('heading', { name: 'Recent sets' })).toBeVisible();
		await expectAccessible(page);
		await page.getByRole('link', { name: 'Press kits', exact: true }).click();
		await expect(page).toHaveURL(/\/projects\/neon-tide\/press-kits$/);
		await expect(page.getByRole('heading', { level: 1, name: 'Neon Tide' })).toBeVisible();
		await expect(page.getByRole('heading', { name: 'Sound' })).toBeVisible();
		await expect(page.getByRole('button', { name: 'Print or save as PDF' })).toBeVisible();
		await expectAccessible(page);
	});

	test('matcher ranks local projects with reasons', async ({ page }) => {
		await page.goto(
			'/match?go=1&place=Berlin&lat=52.52&lng=13.405&radius=50&genres=techno&budget=established&setType=peak'
		);
		const first = page.locator('ol > li').first();
		await expect(first).toContainText('Neon Tide');
		await expect(first).toContainText('Plays techno');
		await expectAccessible(page);
	});

	test('lists upcoming shows everywhere or near a city, and remembers the city', async ({ page }) => {
		await page.goto('/shows');
		await expect(page.getByText('Tidal Hours', { exact: true })).toBeVisible();
		await expect(page.getByText('Neon Tide all night long')).toBeVisible();
		await expectAccessible(page);

		await page.goto('/shows?place=Berlin&lat=52.52&lng=13.405&radius=25');
		await expect(page.getByText('Tidal Hours', { exact: true })).toBeVisible();
		await expect(page.getByText('Neon Tide all night long')).toHaveCount(0);
		await page.goto('/shows');
		await expect(page.getByRole('heading', { name: 'Within 25 km of Berlin' })).toBeVisible();
		await expectAccessible(page);
		await page.getByRole('link', { name: 'Forget this city' }).click();
		await expect(page.getByRole('heading', { name: 'Everywhere' })).toBeVisible();
	});

	test('the charts rank tracks several DJs played, and an artist page lists who played them', async ({ page }) => {
		const tracks = page.getByRole('region', { name: 'Tracks' });

		// The demo sets reach the directory in a backfill after the app first syncs it, so this waits for them.
		await expect(async () => {
			await page.goto('/charts');
			await expect(tracks.getByRole('listitem').first()).toBeVisible({ timeout: 2_000 });
		}).toPass({ timeout: 60_000 });

		await expect(tracks).toContainText('Signal Path');
		// Played by two DJs, one short of charting.
		await expect(tracks).not.toContainText('Static Bloom');
		await expectAccessible(page);

		await page.getByLabel('Where').selectOption('London, GB');
		await page.getByRole('button', { name: 'Show charts' }).click();
		await expect(page).toHaveURL(/place=London/);
		await expect(page.getByText('Nothing charted yet')).toBeVisible();
		await expectAccessible(page);

		await page.goto('/charts');
		await page.getByRole('region', { name: 'Artists' }).getByRole('link', { name: 'Mira Okonkwo' }).click();
		await expect(page).toHaveURL(/\/artists\/7d0b9c12-5e64-4f3a-8b21-c4e9a6f05d72$/);
		await expect(page.getByRole('heading', { level: 1, name: 'Mira Okonkwo' })).toBeVisible();
		const plays = page.getByRole('list', { name: 'Plays of Signal Path' });
		await expect(plays.getByRole('listitem').first()).toContainText('Neon Tide');
		await expect(plays.getByRole('link', { name: 'Pirate Signal' }).first()).toHaveAttribute(
			'href',
			'/projects/pirate-signal'
		);
		await expectAccessible(page);

		await page.goto('/artists?q=okon');
		await expect(page.getByRole('link', { name: 'Mira Okonkwo', exact: true })).toBeVisible();
		await expectAccessible(page);
	});

	test("a profile's sets link their artists to who played them", async ({ page }) => {
		// The sets render with the page once the server has them cached (otherwise they stream in after it
		// hydrates), and a tracklist opened before the page hydrates stays open.
		await page.request.get('/projects/neon-tide');
		await beforeHydration(page, '/projects/neon-tide', () =>
			page.locator('summary', { hasText: 'Tracklist' }).first().click()
		);
		const artist = page.getByRole('link', { name: 'Kestrel Unit' }).first();
		await expect(artist).toBeVisible();
		await artist.click();
		await expect(page).toHaveURL(/\/artists\/name\/Kestrel%20Unit$/);
		await expect(page.getByRole('heading', { level: 1, name: 'Kestrel Unit' })).toBeVisible();
		await expectAccessible(page);
	});

	test("link previews get every public page's title and address without running scripts", async ({ request }) => {
		// What an unfurler reads: the HTML as served, before any script runs.
		const head = async (path: string) => {
			const response = await request.get(path);
			expect(response.status(), path).toBe(200);
			const html = (await response.text()).split('</head>')[0];

			const property = (name: string) =>
				html.match(new RegExp(`<meta (?:property|name)="${name}" content="([^"]*)"`))?.[1];

			return {
				titles: html.match(/<title>/g)?.length ?? 0,
				canonical: html.match(/<link rel="canonical" href="([^"]*)"/)?.[1],
				robots: response.headers()['x-robots-tag'],
				jsonLd: [...html.matchAll(/<script type="application\/ld\+json">(.*?)<\/script>/gs)].map(([, json]) => json),
				property
			};
		};

		for (const path of [
			'/',
			'/about',
			'/for-djs',
			'/privacy',
			'/match',
			'/projects',
			'/shows',
			'/artists',
			'/charts',
			'/tools',
			'/tools/locallibrary',
			'/tools/locallibrary/docs/readme',
			'/projects/neon-tide',
			'/projects/neon-tide/press-kits',
			'/@neon.test'
		]) {
			const page = await head(path);
			expect(page.titles, path).toBe(1);
			expect(page.property('og:title'), path).toBeTruthy();
			expect(page.property('description'), path).toBeTruthy();
			expect(page.property('og:url'), path).toBe(page.canonical);
			expect(new URL(page.canonical ?? '').pathname, path).toBe(path);
			expect(page.robots, path).toBeUndefined();
		}

		// Search engines read who a performer's page is about from its structured data, in the HTML as served.
		const { jsonLd } = await head('/projects/neon-tide');
		expect(jsonLd).toHaveLength(1);
		expect(JSON.parse(jsonLd[0] ?? '')).toMatchObject({
			'@context': 'https://schema.org',
			'@type': 'ProfilePage',
			mainEntity: { '@type': 'MusicGroup', name: 'Neon Tide' }
		});

		// The artist's name reaches the head, not a placeholder written before the plays were counted. The demo sets
		// reach the directory in a backfill after the app first syncs it, so this waits for them.
		await expect(async () => {
			expect((await head('/artists/7d0b9c12-5e64-4f3a-8b21-c4e9a6f05d72')).property('og:title')).toBe('Mira Okonkwo');
		}).toPass({ timeout: 60_000 });

		expect((await request.get('/login')).headers()['x-robots-tag']).toBe('noindex');
	});

	test('staff pages and the dashboard require sign-in', async ({ page, request }) => {
		await page.goto('/admin');
		await expect(page).toHaveURL(/\/login\?next=%2Fadmin/);
		await expectAccessible(page);
		await page.goto('/dashboard/bookings?tab=open');
		await expect(page).toHaveURL(/\/login\?next=%2Fdashboard%2Fbookings%3Ftab%3Dopen$/);

		// Routes match the decoded path, so an encoded one is the same page and needs sign-in too. Sent
		// without a browser: WebKit decodes `%64` to `d` before it sends the request.
		for (const [path, next] of [
			['/%64ashboard/bookings', '%2F%2564ashboard%2Fbookings'],
			['/%61dmin', '%2F%2561dmin']
		]) {
			const response = await request.get(path, { maxRedirects: 0 });
			expect(response.status()).toBe(303);
			expect(response.headers().location).toBe(`/login?next=${next}`);
		}
	});

	test('serves OAuth client metadata and the syncer DID document', async ({ request }) => {
		const metadata = await request.get('/oauth-client-metadata.json');
		const { scope } = await metadata.json();
		expect(scope).toContain('include:computer.localnetwork.permissions');
		// Asked for only when someone first sends a bug report, but the PDS refuses any scope not listed here.
		expect(scope).toContain('repo:app.userinput.discussion?action=create');
		const did = await (await request.get('/.well-known/did.json')).json();
		expect(did.service[0]).toMatchObject({ id: '#localnetwork_syncer', serviceEndpoint: 'http://127.0.0.1:5199' });
	});
});

test.describe('signed in', () => {
	test('a DJ proposes a change and an admin approves it', async ({ page }, testInfo) => {
		// Every project runs against the same network, so each proposes a tagline of its own: the same
		// one again would be no change at all.
		const tagline = `Disco and boogie for sunny afternoons (${testInfo.project.name})`;
		await signIn(page, 'neon.test', 'neon-pass', '/dashboard/projects/saffron-social/profile');
		await expectAccessible(page);
		await page.locator('#tagline').fill(tagline);
		await page.getByRole('button', { name: 'Send for review' }).click();
		await expect(page.getByText('Submitted. A moderator will review it soon')).toBeVisible();
		await expectAccessible(page);
		await signOut(page);

		await signIn(page, 'ada.test', 'ada-pass', '/admin/requests');
		await expectAccessible(page);
		await page.getByRole('link', { name: /Changes: Saffron Social/ }).click();
		await expect(page.getByText(tagline)).toBeVisible();
		await expectAccessible(page);
		await page.getByRole('button', { name: 'Submit decision' }).click();
		await expect(page.getByText('Approved and published')).toBeVisible();

		await page.goto('/admin/audit');
		// The newest entry; earlier projects' approvals read the same.
		await expect(page.getByText("Approved @neon.test's change request for Saffron Social").first()).toBeVisible();
		await page.getByRole('button', { name: 'Verify chain' }).click();
		await expect(page.getByText(/entries verified/)).toBeVisible();
		await expectAccessible(page);

		await page.goto('/projects/saffron-social');
		await expect(page.getByText(tagline)).toBeVisible();
		await expectAccessible(page);
	});

	test('a DJ asks to be listed in Find DJs, staff list them, and they show up there', async ({ page }) => {
		await signIn(page, 'neon.test', 'neon-pass', '/dashboard/profile');
		await expect(page.getByRole('heading', { level: 1, name: 'Your profile' })).toBeVisible();
		await expectAccessible(page);
		await page.getByRole('button', { name: 'Ask to be listed' }).click();
		await expect(page.getByText('Submitted. A moderator will review it soon')).toBeVisible();
		await signOut(page);

		await signIn(page, 'ada.test', 'ada-pass', '/admin/requests?kind=list');
		await page.getByRole('link', { name: 'Listing: @neon.test' }).first().click();
		await expect(page.getByRole('heading', { name: 'Profile to list' })).toBeVisible();
		// neon.test manages Neon Tide alone, so its card gives way to their profile once they're listed.
		await expect(page.getByText(/They also manage Neon Tide, a solo project/)).toBeVisible();
		await expectAccessible(page);
		await page.getByRole('button', { name: 'Submit decision' }).click();
		await expect(page.getByText('Approved and listed in Find DJs')).toBeVisible();

		await page.goto('/projects?q=neon');
		await expect(page.getByRole('link', { name: 'Neon', exact: true })).toHaveAttribute('href', '/@neon.test');
		await expect(page.getByRole('link', { name: 'Neon Tide', exact: true })).toHaveCount(0);
		await expectAccessible(page);

		// Staff's Manage link on the profile goes to the listing, which takes them out again for the next browser's run.
		await page.goto('/@neon.test');
		await page.getByRole('link', { name: 'Manage' }).click();
		await expect(page.getByText(/Listed in Find DJs since/)).toBeVisible();
		await expectAccessible(page);
		page.once('dialog', (dialog) => dialog.accept());
		await page.getByRole('button', { name: 'Take out of Find DJs' }).click();
		await expect(page.getByText('Taken out of Find DJs')).toBeVisible();
		await page.goto('/projects?q=neon');
		await expect(page.getByRole('link', { name: 'Neon Tide', exact: true })).toBeVisible();
	});

	test("a project's manager goes from its public page to its dashboard, and others can't", async ({ page }) => {
		await signIn(page, 'neon.test', 'neon-pass', '/projects/neon-tide');
		await page.getByRole('link', { name: 'Manage', exact: true }).click();
		await expect(page).toHaveURL('/dashboard/projects/neon-tide');
		await expect(page.getByRole('heading', { level: 1, name: 'Neon Tide' })).toBeVisible();
		await expectAccessible(page);
		await signOut(page);

		await signIn(page, 'pat.test', 'pat-pass', '/projects/neon-tide');
		await expect(page.getByRole('link', { name: 'Manage', exact: true })).toHaveCount(0);
		await page.goto('/dashboard/projects/neon-tide');
		await expect(page.getByText("Only this project's managers can manage it")).toBeVisible();
		// The staff console turns non-staff away before any of its pages load.
		const admin = await page.goto('/admin/requests');
		expect(admin?.status()).toBe(403);
		await expect(page.getByText('Only moderators and admins can see this page')).toBeVisible();

		// SvelteKit routes an encoded path to the same pages, so it's turned away just the same. A data
		// request shows it: past a refusing layout, it would still carry the page's own data.
		for (const path of ['/%61dmin/__data.json', '/adm%69n/requests/__data.json', '/%61dmin/audit/__data.json']) {
			const response = await page.request.get(path, { maxRedirects: 0 });
			expect(response.status(), path).toBe(403);
			expect(await response.text(), path).not.toContain('"type":"data"');
		}

		await signOut(page);
	});

	test("an admin verifies a project's manager on Bluesky, then removes it", async ({ page }) => {
		await signIn(page, 'ada.test', 'ada-pass', '/admin/projects/neon-tide');
		const managers = page.locator('form[action="?/managers"]');
		await managers.getByRole('link', { name: 'Verify' }).click();
		// The manager's handle, or their DID until the app has learned the handle.
		await expect(page.getByLabel('Handle or DID')).not.toHaveValue('');
		await expectAccessible(page);
		await page.getByRole('button', { name: 'Verify', exact: true }).click();
		await expect(page.getByText('Verified @neon.test as “Neon”')).toBeVisible();
		const row = page.getByRole('row', { name: /@neon\.test/ });
		await expect(row.getByText('Verified', { exact: true })).toBeVisible();
		await expectAccessible(page);

		await page.goto('/admin/projects/neon-tide');
		await expect(managers.getByText('Verified', { exact: true })).toBeVisible();

		await page.goto('/admin/verifications');
		page.once('dialog', (dialog) => dialog.accept());
		await row.getByRole('button', { name: 'Remove' }).click();
		await expect(page.getByText('Verification removed')).toBeVisible();
		await expect(row).toHaveCount(0);
	});

	test('someone asks to see a press kit, and a manager lets them in', async ({ page }) => {
		await signIn(page, 'milo.test', 'milo-pass', '/projects/neon-tide/press-kits');
		// The old list address, from links in emails, still lands on the list.
		await page.goto('/projects/neon-tide/epk');
		await expect(page).toHaveURL('/projects/neon-tide/press-kits');
		await page.getByRole('link', { name: /Clubs/ }).click();
		await expect(page.getByRole('heading', { name: 'The rest of this press kit' })).toBeVisible();
		await expect(page.getByText('bookings@neon-tide.test')).toHaveCount(0);
		await expectAccessible(page);
		const kit = new URL(page.url()).pathname;
		const rkey = kit.split('/').at(-1)!;
		expect(kit).toBe(`/projects/neon-tide/press-kits/${rkey}`);
		const access = `/dashboard/projects/neon-tide/press-kits/${rkey}/access`;

		// An old link named the repo that held the press kit, here its maker's; it still opens the press kit.
		const resolved = await page.request.get(
			'http://localhost:2683/xrpc/com.atproto.identity.resolveHandle?handle=neon.test'
		);

		const { did } = z.object({ did: z.string() }).parse(await resolved.json());
		await page.goto(`/epk/${did}/${rkey}`);
		await expect(page).toHaveURL(kit);

		// The public page only links to the form, which is on the dashboard.
		await page.getByRole('link', { name: 'Ask for access' }).click();
		await expect(page).toHaveURL(`/dashboard/access/neon-tide/${rkey}`);
		await expectAccessible(page);
		await page.getByLabel("You're a").selectOption('press');
		await page.getByLabel(/Message/).fill('Writing about Berlin residents');
		await page.getByRole('button', { name: 'Ask for access' }).click();
		await expect(page.getByText(/You asked on/)).toBeVisible();
		await page.goto(kit);
		await expect(page.getByText(/You asked on/)).toBeVisible();
		await signOut(page);

		await signIn(page, 'neon.test', 'neon-pass', access);
		await expectAccessible(page);
		await page
			.getByRole('listitem')
			.filter({ hasText: 'Writing about Berlin residents' })
			.getByRole('button', { name: 'Let in' })
			.click();
		await expect(page.getByText('Approved. They can see the whole press kit now.')).toBeVisible();
		await signOut(page);

		await signIn(page, 'milo.test', 'milo-pass', kit);
		await expect(page.getByText('bookings@neon-tide.test')).toBeVisible();
		await expect(page.getByRole('link', { name: 'Halle Nord closing set (promo)' })).toBeVisible();
		await expectAccessible(page);
		await signOut(page);

		// Each browser runs against the same network, so the manager takes access away again.
		await signIn(page, 'neon.test', 'neon-pass', access);
		await page.getByRole('listitem').filter({ hasText: '@milo.test' }).getByRole('button', { name: 'Remove' }).click();
		await expect(page.getByText('Access removed.')).toBeVisible();
	});

	test('a fan follows a DJ and sees their shows under Following', async ({ page }) => {
		// milo.test follows Neon Tide from the seed data.
		await signIn(page, 'milo.test', 'milo-pass', '/projects/saffron-social');
		await page.getByRole('button', { name: /^Follow \d/ }).click();
		await expect(page.getByRole('button', { name: /^Following \d/ })).toHaveAttribute('aria-pressed', 'true');
		await expectAccessible(page);

		await page.goto('/shows?view=following');
		await expect(page.getByText('Saffron Social on the roof')).toBeVisible();
		await expect(page.getByText('Tidal Hours', { exact: true })).toBeVisible();
		await expectAccessible(page);

		// Each browser runs against the same network, so the fan unfollows again.
		await page.goto('/projects/saffron-social');
		await page.getByRole('button', { name: /^Following \d/ }).click();
		await expect(page.getByRole('button', { name: /^Follow \d/ })).toHaveAttribute('aria-pressed', 'false');
	});

	test('a DJ announces a show, cancels it, and deletes it', async ({ page }, testInfo) => {
		const name = `Basement rave (${testInfo.project.name})`;
		const date = new Date(Date.now() + 30 * 86_400_000).toISOString().slice(0, 10);
		await signIn(page, 'neon.test', 'neon-pass', '/dashboard/projects/neon-tide/shows/new');
		await expectAccessible(page);
		await page.locator('#name').fill(name);
		await page.locator('#date').fill(date);
		await page.locator('#start').fill('23:00');
		await page.locator('#end').fill('05:00');
		await page.locator('#city').fill('Berlin');
		await page.locator('#country').fill('DE');
		await page.getByRole('button', { name: 'Announce show' }).click();
		await expect(page.getByRole('alert')).toContainText("Pick the venue's city from the search");
		// The failed save is a full page load, and hydrating the form resets its coordinates, so wait for that first.
		await expect(page.locator('html')).toHaveAttribute('data-hydrated', '');
		// The e2e network has no geocoder, so this stands in for picking Berlin from the city search.
		await page.locator('input[name="latE6"]').evaluate((input: HTMLInputElement) => (input.value = '52520000'));
		await page.locator('input[name="lngE6"]').evaluate((input: HTMLInputElement) => (input.value = '13405000'));
		await page.getByRole('button', { name: 'Announce show' }).click();
		const listed = page.getByRole('listitem').filter({ hasText: name });
		await expect(listed).toContainText('23:00 to 05:00');
		await expectAccessible(page);

		await listed.getByRole('link', { name: `Edit ${name}` }).click();
		await page.getByLabel('This show is cancelled').check();
		await page.getByRole('button', { name: 'Save changes' }).click();
		await expect(listed).toContainText('Cancelled');

		await listed.getByRole('link', { name: `Edit ${name}` }).click();
		await page.getByRole('button', { name: 'Delete show' }).click();
		await expect(page.getByRole('heading', { name: 'Upcoming shows' })).toBeVisible();
		await expect(listed).toHaveCount(0);
	});

	test('a promoter sends a booking inquiry', async ({ page }) => {
		await signIn(page, 'pat.test', 'pat-pass', '/dashboard/bookings/new?project=neon-tide');
		await expect(page.getByRole('heading', { name: 'Neon Tide' })).toBeVisible();
		await expectAccessible(page);
		await page.locator('#title').fill('Basement Sessions');
		await page.locator('#date').fill('2026-12-12');
		await page.locator('#message').fill('Would you play our December basement night?');
		await page.getByRole('button', { name: 'Send inquiry' }).click();
		await expect(page.getByRole('heading', { name: 'Basement Sessions' })).toBeVisible();
		await page.locator('#text').fill('Budget is flexible.');
		await page.getByRole('button', { name: 'Send' }).click();
		await expect(page.getByText('Budget is flexible.')).toBeVisible();
		await expectAccessible(page);
		const thread = new URL(page.url()).pathname;
		await signOut(page);

		// Once it's confirmed, the DJ can announce it, starting from the inquiry's details.
		await signIn(page, 'neon.test', 'neon-pass', thread);
		await expect(page.getByRole('link', { name: 'Announce this show' })).toHaveCount(0);
		await page.getByLabel('Status').selectOption('confirmed');
		await page.getByRole('button', { name: 'Update status' }).click();
		await page.getByRole('link', { name: 'Announce this show' }).click();
		await expect(page.locator('#name')).toHaveValue('Basement Sessions');
		await expect(page.locator('#date')).toHaveValue('2026-12-12');
		await expectAccessible(page);
	});
});

test.describe('bug reports', () => {
	test("signed out, Report a bug opens GitHub's form with the page and browser filled in", async ({
		page,
		request
	}) => {
		await page.goto('/match');
		const link = page.getByRole('contentinfo').getByRole('link', { name: 'Report a bug' });
		await expect(link).toHaveAttribute('href', '/report?from=%2Fmatch');
		// Followed by hand, so the test never loads GitHub.
		const response = await request.get('/report?from=%2Fmatch', { maxRedirects: 0 });
		const github = new URL(response.headers().location);
		expect(github.origin + github.pathname).toBe('https://github.com/cutenode/localnetwork/issues/new');
		expect(github.searchParams.get('template')).toBe('web-bug.yml');
		expect(github.searchParams.get('steps')).toContain('/match');
	});

	test('signed in, someone approves posting once and reports a bug from their own account', async ({ page }) => {
		// A fresh sign-in asks only for what the site needs, so posting has to be approved first.
		await signIn(page, 'pat.test', 'pat-pass', '/match');
		await page.getByRole('contentinfo').getByRole('link', { name: 'Report a bug' }).click();
		await expect(page.getByRole('heading', { name: 'Approve posting first' })).toBeVisible();
		await expectAccessible(page);
		await page.getByRole('button', { name: 'Approve' }).click();
		// The PDS may ask for the password again before it shows what's being approved.
		const password = page.locator('input[type="password"]');
		const authorize = page.getByRole('button', { name: 'Authorize' });
		await expect(password.or(authorize).first()).toBeVisible();

		if (await password.isVisible()) {
			await password.fill('pat-pass');
			await page.getByRole('button', { name: 'Sign in' }).click();
		}

		await authorize.click();
		await page.waitForURL((url) => url.port === '5199' && url.pathname === '/report');
		await page.waitForLoadState('networkidle');
		await expect(page.getByText('Also sent: /match')).toBeVisible();
		await expectAccessible(page);
		await page.getByRole('textbox', { name: 'Title' }).fill("The map doesn't load on a show");
		await page.getByRole('textbox', { name: 'What happened?' }).fill('I added a venue and the map stayed grey.');
		await page.getByRole('button', { name: 'Post report' }).click();
		await expect(page.getByRole('heading', { name: "Thanks, it's posted" })).toBeVisible();
		await expect(page.getByRole('link', { name: 'View your report' })).toHaveAttribute(
			'href',
			/^https:\/\/userinput\.app\/d\/did:plc:[a-z0-9]+\/[a-z0-9]+$/
		);
		await expectAccessible(page);
		await signOut(page);
	});
});

test.describe('keeps what people entered', () => {
	// A 1×1 PNG.
	const png = Buffer.from(
		'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==',
		'base64'
	);

	test('a DJ with changes waiting goes back to them and sees the proposed photo', async ({ page }) => {
		await signIn(page, 'neon.test', 'neon-pass', '/dashboard/projects/neon-tide/profile');
		await page.locator('#avatar').setInputFiles({ name: 'photo.png', mimeType: 'image/png', buffer: png });
		await page.getByRole('button', { name: 'Send for review' }).click();
		await expect(page.getByText('Submitted. A moderator will review it soon')).toBeVisible();
		const request = new URL(page.url()).pathname;

		await page.goto('/dashboard/projects/neon-tide/profile');
		await expect(page).toHaveURL(request);
		await page.waitForLoadState('networkidle');
		await page.getByRole('button', { name: 'Edit request' }).click();
		const photo = page.locator('form img');
		await expect(photo).toBeVisible();
		await expectAccessible(page);
		expect((await page.request.get((await photo.getAttribute('src'))!)).ok()).toBe(true);
		await page.getByLabel('Remove the current image').check();
		await page.getByRole('button', { name: 'Save and resubmit' }).click();
		// The editor stays open after a revise, now without a photo.
		await expect(page.getByLabel('Remove the current image')).toHaveCount(0);
		await expect(photo).toHaveCount(0);

		page.once('dialog', (dialog) => dialog.accept());
		await page.getByRole('button', { name: 'Withdraw' }).click();
		await expect(page.getByText('Request withdrawn')).toBeVisible();
	});

	test('the profile and press kit editors keep what was entered before the page hydrated', async ({ page }) => {
		await signIn(page, 'neon.test', 'neon-pass');

		const tagline = page.getByLabel('Tagline');
		const bio = page.getByLabel('Bio', { exact: true });
		const modular = page.locator('input[name="formats"][value="modular"]');
		const fee = page.getByLabel('What this fee covers').first();

		await beforeHydration(page, '/dashboard/projects/neon-tide/profile', async () => {
			await tagline.fill('Typed before the script loaded');
			await bio.fill('A bio typed early');
			await modular.locator('xpath=..').click();
			await fee.fill('Warehouse nights');
		});
		await expect(tagline).toHaveValue('Typed before the script loaded');
		await expect(bio).toHaveValue('A bio typed early');
		await expect(modular).toBeChecked();
		await expect(fee).toHaveValue('Warehouse nights');

		const kitHeadline = page.getByLabel('Headline');
		const longBio = page.getByLabel('Full bio');
		const open = page.getByRole('radio', { name: /^Open/ });
		const quote = page.getByLabel('Quote 1', { exact: true });

		await beforeHydration(page, '/dashboard/projects/neon-tide/press-kits/new', async () => {
			await kitHeadline.fill('Typed before the script loaded');
			await longBio.fill('A full bio typed early');
			await open.check();
			await quote.fill('Relentless');
		});
		await expect(kitHeadline).toHaveValue('Typed before the script loaded');
		await expect(longBio).toHaveValue('A full bio typed early');
		await expect(open).toBeChecked();
		await expect(quote).toHaveValue('Relentless');
		// Picking a preset before the script loaded still sets the rules it stands for.
		await expect(page.getByRole('radio', { name: 'Anyone signed in: Approved automatically' })).toBeChecked();
	});

	test('a claim can change its relationship and links when revised', async ({ page }) => {
		await signIn(page, 'pat.test', 'pat-pass', '/dashboard/claim/halflight');
		await expectAccessible(page);
		await page.getByLabel("I'm their booking agent").check();
		await page.locator('#evidence').fill('I book them; see the bio link');
		await page.locator('#links').fill('https://example.test/bio');
		await page.getByRole('button', { name: 'Submit claim' }).click();
		await expect(page.getByText('Submitted. A moderator will review it soon')).toBeVisible();
		await page.waitForLoadState('networkidle');

		await page.getByRole('button', { name: 'Edit request' }).click();
		await expect(page.locator('#links')).toHaveValue('https://example.test/bio');
		await page.getByLabel("I'm their manager").check();
		await page.locator('#links').fill('https://example.test/bio\nhttps://example.test/roster');
		await page.getByRole('button', { name: 'Save and resubmit' }).click();
		await expect(page.getByText("Relationship: I'm their manager")).toBeVisible();
		await expect(page.getByRole('link', { name: 'https://example.test/roster' })).toBeVisible();
		await expectAccessible(page);
		await page.waitForLoadState('networkidle');

		page.once('dialog', (dialog) => dialog.accept());
		await page.getByRole('button', { name: 'Withdraw' }).click();
		await expect(page.getByText('Request withdrawn')).toBeVisible();
	});

	test('the planner keeps a show that could not be saved, within the record limits', async ({ page }) => {
		await signIn(page, 'pat.test', 'pat-pass', '/dashboard/planner/shows/new');
		await expect(page.locator('#title')).toHaveAttribute('maxlength', '120');
		await expect(page.locator('#venue')).toHaveAttribute('maxlength', '120');
		await expect(page.locator('#notes')).toHaveAttribute('maxlength', '2000');
		await page.locator('#title').fill('Echo Night');
		await page.locator('#venue').fill('The Lot');
		await page.locator('#capacity').fill('2000000');
		await page.locator('#notes').fill('Bring records');
		await page.getByRole('button', { name: 'Save show' }).click();
		await expect(page.getByRole('alert')).toContainText('capacity');
		await expectAccessible(page);
		await expect(page.locator('#title')).toHaveValue('Echo Night');
		await expect(page.locator('#venue')).toHaveValue('The Lot');
		await expect(page.locator('#notes')).toHaveValue('Bring records');
	});

	test('an inquiry from a planner show starts from its details, and keeps an unsent message', async ({ page }) => {
		await signIn(page, 'pat.test', 'pat-pass');
		await page.goto('/match?go=1&title=Rooftop%20Rave&date=2026-12-19&lat=52.52&lng=13.405&radius=50&genres=techno');
		await page.waitForLoadState('networkidle');
		await page.locator('ol > li').filter({ hasText: 'Neon Tide' }).getByLabel('Lineup').check();
		await page.getByRole('button', { name: 'Save show to planner' }).click();
		await page.getByRole('link', { name: 'Inquire with Neon Tide' }).click();
		await expect(page.locator('#title')).toHaveValue('Rooftop Rave');
		await expect(page.locator('#date')).toHaveValue('2026-12-19');
		await expect(page.getByText('in your planner')).toBeVisible();

		await expect(page.locator('html')).toHaveAttribute('data-hydrated', '');
		await page.locator('#message').fill('Would you close our rooftop night?');
		await page.reload();
		await expect(page.locator('#message')).toHaveValue('Would you close our rooftop night?');
		await expect(page.getByRole('status').filter({ hasText: 'unsent changes' })).toBeVisible();
		await expectAccessible(page);

		// Signing out drops it, so the next person on this browser doesn't find it, even when Sign out is
		// pressed before the dashboard's scripts have run.
		const inquiry = page.url();

		const holdScripts = (route: Route) =>
			route.request().resourceType() === 'script' ? route.abort() : route.fallback();

		await page.route('**/*', holdScripts);
		await page.goto('/dashboard');
		await page.unroute('**/*', holdScripts);
		await expect(page.locator('html')).not.toHaveAttribute('data-hydrated');
		await page.getByRole('button', { name: 'Sign out' }).click();
		await page.waitForURL((url) => url.pathname === '/');
		await expect(page.locator('html')).toHaveAttribute('data-hydrated', '');
		const drafts = () => Object.keys(localStorage).filter((key) => key.startsWith('ln:draft:'));
		expect(await page.evaluate(drafts)).toEqual([]);
		await signIn(page, 'pat.test', 'pat-pass', new URL(inquiry).pathname + new URL(inquiry).search);
		await expect(page.locator('#title')).toHaveValue('Rooftop Rave');
		await page.locator('#message').fill('Would you close our rooftop night?');

		await page.getByRole('button', { name: 'Send inquiry' }).click();
		await expect(page.getByRole('heading', { name: 'Rooftop Rave' })).toBeVisible();
		// Sent, the draft is gone.
		await page.goto(inquiry);
		await expect(page.locator('html')).toHaveAttribute('data-hydrated', '');
		await expect(page.locator('#message')).toHaveValue('');
	});

	test('a failed save from the matcher keeps the search and the lineup picks', async ({ page }) => {
		const place = `${'Long venue name '.repeat(8)}Berlin`;
		await signIn(page, 'pat.test', 'pat-pass');
		await page.goto(`/match?go=1&place=${encodeURIComponent(place)}&lat=52.52&lng=13.405&radius=50&genres=techno`);
		await page.waitForLoadState('networkidle');
		const first = page.locator('ol > li').first();
		await expect(first).toContainText('Neon Tide');
		await first.getByLabel('Lineup').check();
		await page.getByRole('button', { name: 'Save show to planner' }).click();
		await expect(page.getByRole('alert')).toBeVisible();
		await expect(first).toContainText('Neon Tide');
		await expect(first.getByLabel('Lineup')).toBeChecked();
		await expectAccessible(page);
	});
});
