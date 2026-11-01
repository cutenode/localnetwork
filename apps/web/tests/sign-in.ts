import type { Page } from 'playwright';

/**
 * Signs in through the local PDS's OAuth screens with a seeded test account (scripts/dev-network.ts),
 * for the end-to-end tests and Lighthouse's signed-in pages. The page's context needs a baseURL.
 */
export async function signIn(page: Page, handle: string, password: string, next = '/dashboard') {
	await page.goto(`/login?next=${encodeURIComponent(next)}`);
	const app = new URL(page.url());
	await page.waitForLoadState('networkidle');
	await page.getByPlaceholder('alice.bsky.social').fill(handle);
	await page.getByRole('button', { name: 'Continue' }).click();
	await page.locator('input[type="password"]').fill(password);
	await page.getByRole('button', { name: 'Sign in' }).click();
	await page.getByRole('button', { name: 'Authorize' }).click();
	const target = new URL(next, app);
	await page.waitForURL((url) => url.origin === app.origin && url.pathname === target.pathname);
	await page.waitForLoadState('networkidle');
}
