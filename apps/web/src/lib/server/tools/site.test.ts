import { afterEach, describe, expect, it } from 'vitest';
import publishing from '../../../../../../lexicons/computer/localnetwork/locallibraryPublishing.json' with { type: 'json' };
import { toolSite } from './site';

const request = (host: string, method = 'GET', headers: Record<string, string> = {}) =>
	new Request(`https://${host}/`, { method, headers: { host, ...headers } });

const HOST = 'locallibrary.localnetwork.computer';

describe('toolSite', () => {
	afterEach(() => {
		delete process.env.LOCALLIBRARY_HOST;
		delete process.env.PUBLIC_URL;
	});

	it('serves the client document servers fetch while signing in', async () => {
		const res = toolSite(request(HOST), '/oauth-client-metadata.json');
		expect(res?.status).toBe(200);
		expect(res?.headers.get('content-type')).toBe('application/json');
		expect(res?.headers.get('access-control-allow-origin')).toBe('*');
		expect(res?.headers.get('cache-control')).toBe('public, max-age=600');
		expect(res?.headers.get('x-content-type-options')).toBe('nosniff');
		const doc = await res!.json();
		// What locallibrary's sign-in checks before using it.
		expect(doc.client_id).toBe('https://locallibrary.localnetwork.computer/oauth-client-metadata.json');
		expect(doc.application_type).toBe('native');
		expect(doc.token_endpoint_auth_method).toBe('none');
		expect(doc.dpop_bound_access_tokens).toBe(true);
		expect(doc.redirect_uris).toContain('http://127.0.0.1/callback');
		// Servers refuse a sign-in that asks for more than this allows, so publishing needs its permission set here.
		expect(doc.scope.split(' ')).toEqual(expect.arrayContaining(['atproto', `include:${publishing.id}`]));
		const head = toolSite(request(HOST, 'HEAD'), '/oauth-client-metadata.json');
		expect(head?.status).toBe(200);
		expect(await head!.text()).toBe('');
	});

	it("sends the root to the tool's page on the main site", async () => {
		const res = toolSite(request('LocalLibrary.localnetwork.computer:443'), '/');
		expect(res?.status).toBe(308);
		expect(res?.headers.get('location')).toBe('https://localnetwork.computer/tools/locallibrary');
		const head = toolSite(request(HOST, 'HEAD'), '/');
		expect(head?.status).toBe(308);
		expect(await head!.text()).toBe('');
		process.env.PUBLIC_URL = 'https://staging.example/';
		expect(toolSite(request(HOST), '/')?.headers.get('location')).toBe('https://staging.example/tools/locallibrary');
		process.env.PUBLIC_URL = 'not a url';
		expect(toolSite(request(HOST), '/')?.headers.get('location')).toBe(
			'https://localnetwork.computer/tools/locallibrary'
		);
	});

	it('answers nothing else on its host', () => {
		expect(toolSite(request(HOST), '/login')?.status).toBe(404);
		expect(toolSite(request(HOST), '/tools/locallibrary')?.status).toBe(404);
		const post = toolSite(request(HOST, 'POST'), '/');
		expect(post?.status).toBe(405);
		expect(post?.headers.get('allow')).toBe('GET, HEAD');
	});

	it('leaves every other host to the app', () => {
		expect(toolSite(request('localnetwork.computer'), '/oauth-client-metadata.json')).toBeNull();
		expect(toolSite(request('evil.example'), '/')).toBeNull();
		expect(toolSite(new Request('https://localnetwork.computer/'), '/')).toBeNull();
	});

	it('follows the proxy and the configured host', () => {
		const proxied = request('10.0.0.5:3000', 'GET', { 'x-forwarded-host': HOST });
		expect(toolSite(proxied, '/')?.status).toBe(308);
		process.env.LOCALLIBRARY_HOST = 'locallibrary.staging.example';
		expect(toolSite(request('locallibrary.staging.example'), '/oauth-client-metadata.json')?.status).toBe(200);
		expect(toolSite(request(HOST), '/')).toBeNull();
	});
});
