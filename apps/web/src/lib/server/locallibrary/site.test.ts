import { afterEach, describe, expect, it } from 'vitest';
import { locallibraryHost, locallibrarySite } from './site';

const request = (host: string, method = 'GET', headers: Record<string, string> = {}) =>
	new Request(`https://${host}/`, { method, headers: { host, ...headers } });

describe('locallibrarySite', () => {
	afterEach(() => {
		delete process.env.LOCALLIBRARY_HOST;
	});

	it('serves the client document servers fetch while signing in', async () => {
		const res = locallibrarySite(request('locallibrary.localnetwork.computer'), '/oauth-client-metadata.json');
		expect(res?.status).toBe(200);
		expect(res?.headers.get('content-type')).toBe('application/json');
		const doc = await res!.json();
		// What locallibrary's sign-in checks before using it.
		expect(doc.client_id).toBe('https://locallibrary.localnetwork.computer/oauth-client-metadata.json');
		expect(doc.application_type).toBe('native');
		expect(doc.token_endpoint_auth_method).toBe('none');
		expect(doc.dpop_bound_access_tokens).toBe(true);
		expect(doc.redirect_uris).toContain('http://127.0.0.1/callback');
		// Servers refuse a sign-in that asks for more than this allows, so publishing needs its scopes here.
		expect(doc.scope.split(' ')).toEqual(
			expect.arrayContaining([
				'atproto',
				...['locallibrary.settings', 'dj.profile', 'dj.set'].map(
					(record) => `repo:computer.localnetwork.${record}?action=create&action=update`
				)
			])
		);
	});

	it('serves the page at the root, locked down', async () => {
		const res = locallibrarySite(request('LocalLibrary.localnetwork.computer:443'), '/');
		expect(res?.status).toBe(200);
		expect(res?.headers.get('content-security-policy')).toContain("default-src 'none'");
		const html = await res!.text();
		expect(html).toMatch(/Atmosphere\s+account/);
		// It says what publishing asks for, since the client document now allows it.
		expect(html).toMatch(/can't delete them/);
		const head = locallibrarySite(request('locallibrary.localnetwork.computer', 'HEAD'), '/');
		expect(head?.status).toBe(200);
		expect(await head!.text()).toBe('');
	});

	it('answers nothing else on its host', () => {
		const host = 'locallibrary.localnetwork.computer';
		expect(locallibrarySite(request(host), '/login')?.status).toBe(404);
		expect(locallibrarySite(request(host, 'POST'), '/')?.status).toBe(405);
	});

	it('leaves every other host to the app', () => {
		expect(locallibrarySite(request('localnetwork.computer'), '/oauth-client-metadata.json')).toBeNull();
		expect(locallibrarySite(request('evil.example'), '/')).toBeNull();
	});

	it('follows the proxy and the configured host', () => {
		const proxied = request('10.0.0.5:3000', 'GET', { 'x-forwarded-host': 'locallibrary.localnetwork.computer' });
		expect(locallibrarySite(proxied, '/')?.status).toBe(200);
		process.env.LOCALLIBRARY_HOST = 'locallibrary.staging.example';
		expect(locallibraryHost()).toBe('locallibrary.staging.example');
		expect(locallibrarySite(request('locallibrary.staging.example'), '/')?.status).toBe(200);
	});
});
