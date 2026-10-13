import { expect, type Page, test } from '@playwright/test';
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

/** Signs in through the local PDS's OAuth screens with a seeded test account. */
async function signIn(page: Page, handle: string, password: string, next = '/me') {
	await page.goto(`/login?next=${encodeURIComponent(next)}`);
	await page.waitForLoadState('networkidle');
	await page.getByPlaceholder('alice.bsky.social').fill(handle);
	await page.getByRole('button', { name: 'Continue' }).click();
	await page.locator('input[type="password"]').fill(password);
	await page.getByRole('button', { name: 'Sign in' }).click();
	await page.getByRole('button', { name: 'Authorize' }).click();
	const target = new URL(next, 'http://127.0.0.1:5199');
	await page.waitForURL((url) => url.port === '5199' && url.pathname === target.pathname);
	await page.waitForLoadState('networkidle');
}

/**
 * Signs out from /me and waits for the redirect home: /logout records the audit entry and revokes
 * the OAuth session first, and a sign-in started before it finishes would find the old session.
 */
async function signOut(page: Page) {
	await page.goto('/me');
	await page.getByRole('button', { name: 'Sign out' }).click();
	await page.waitForURL((url) => url.pathname === '/');
}

test.describe('public pages', () => {
	test('home page lists seeded projects and links to search', async ({ page }) => {
		await page.goto('/');
		await expect(page.getByRole('heading', { level: 1 })).toContainText('The home for local DJs');
		await expect(page.getByRole('heading', { name: 'New on the directory' })).toBeVisible();
		await expectAccessible(page);
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
		await expect(page.getByText('Sign in to see booking contacts')).toBeVisible();
		await expectAccessible(page);
	});

	test('a profile shows how the DJ plays, and its press kit prints', async ({ page }) => {
		await page.goto('/projects/neon-tide');
		await expect(page.getByRole('heading', { name: 'How they play' })).toBeVisible();
		await expect(page.getByText('126–134')).toBeVisible();
		await expect(page.getByRole('heading', { name: 'Recent sets' })).toBeVisible();
		await expectAccessible(page);
		await page.getByRole('link', { name: 'Press kit' }).click();
		await expect(page).toHaveURL(/\/projects\/neon-tide\/epk$/);
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

	test('staff pages require sign-in', async ({ page }) => {
		await page.goto('/admin');
		await expect(page).toHaveURL(/\/login\?next=%2Fadmin/);
		await expectAccessible(page);
	});

	test('serves OAuth client metadata and the syncer DID document', async ({ request }) => {
		const metadata = await request.get('/oauth-client-metadata.json');
		expect((await metadata.json()).scope).toContain('include:computer.localnetwork.permissions');
		const did = await (await request.get('/.well-known/did.json')).json();
		expect(did.service[0]).toMatchObject({ id: '#localnetwork_syncer', serviceEndpoint: 'http://127.0.0.1:5199' });
	});
});

test.describe('signed in', () => {
	test('a DJ proposes a change and an admin approves it', async ({ page }, testInfo) => {
		// Every project runs against the same network, so each proposes a tagline of its own: the same
		// one again would be no change at all.
		const tagline = `Disco and boogie for sunny afternoons (${testInfo.project.name})`;
		await signIn(page, 'neon.test', 'neon-pass', '/projects/saffron-social/edit');
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

	test('someone asks to see a press kit, and its owner lets them in', async ({ page }) => {
		await signIn(page, 'milo.test', 'milo-pass', '/projects/neon-tide/epk');
		await page.getByRole('link', { name: /Clubs/ }).click();
		await expect(page.getByRole('heading', { name: 'The rest of this press kit' })).toBeVisible();
		await expect(page.getByText('bookings@neon-tide.test')).toHaveCount(0);
		await expectAccessible(page);
		await page.getByLabel("You're a").selectOption('press');
		await page.getByLabel(/Message/).fill('Writing about Berlin residents');
		await page.getByRole('button', { name: 'Ask for access' }).click();
		await expect(page.getByText(/You asked on/)).toBeVisible();
		const kit = new URL(page.url()).pathname;
		await signOut(page);

		await signIn(page, 'neon.test', 'neon-pass', `${kit}/access`);
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

		// Each browser runs against the same network, so the owner takes access away again.
		await signIn(page, 'neon.test', 'neon-pass', `${kit}/access`);
		await page.getByRole('listitem').filter({ hasText: '@milo.test' }).getByRole('button', { name: 'Remove' }).click();
		await expect(page.getByText('Access removed.')).toBeVisible();
	});

	test('a promoter sends a booking inquiry', async ({ page }) => {
		await signIn(page, 'pat.test', 'pat-pass', '/bookings/new?project=neon-tide');
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
	});
});

test.describe('keeps what people entered', () => {
	// A 1×1 PNG.
	const png = Buffer.from(
		'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==',
		'base64'
	);

	test('a DJ with changes waiting goes back to them and sees the proposed photo', async ({ page }) => {
		await signIn(page, 'neon.test', 'neon-pass', '/projects/neon-tide/edit');
		await page.locator('#avatar').setInputFiles({ name: 'photo.png', mimeType: 'image/png', buffer: png });
		await page.getByRole('button', { name: 'Send for review' }).click();
		await expect(page.getByText('Submitted. A moderator will review it soon')).toBeVisible();
		const request = new URL(page.url()).pathname;

		await page.goto('/projects/neon-tide/edit');
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

	test('a claim can change its relationship and links when revised', async ({ page }) => {
		await signIn(page, 'pat.test', 'pat-pass', '/projects/halflight/claim');
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
		await signIn(page, 'pat.test', 'pat-pass', '/planner/shows/new');
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

	test('a failed save from the matcher keeps the search and the lineup picks', async ({ page }) => {
		const place = `${'Long venue name '.repeat(8)}Berlin`;
		await signIn(page, 'pat.test', 'pat-pass', '/me');
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
