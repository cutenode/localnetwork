import { describe, expect, it } from 'vitest';
import { loadConfig, serviceDidFor } from './config';

const base = {
	ROOT_IDENTIFIER: 'localnetwork.computer',
	ROOT_PASSWORD: 'secret',
	HAPPYVIEW_URL: 'https://happyview.example/'
};

describe('loadConfig', () => {
	it('applies development defaults', () => {
		const config = loadConfig(base);
		expect(config).toMatchObject({
			env: 'development',
			dev: true,
			publicUrl: 'http://127.0.0.1:5173',
			serviceDid: 'did:web:localhost%3A5173',
			syncService: 'did:web:localhost%3A5173#localnetwork_syncer',
			allowPrivateNetwork: true,
			syncNotify: true,
			logLevel: 'debug',
			adminDids: [],
			epkViewRetentionDays: 90,
			happyView: { url: 'https://happyview.example', apiKey: undefined }
		});
	});

	it('locks production down', () => {
		const config = loadConfig({
			...base,
			NODE_ENV: 'production',
			PUBLIC_URL: 'https://ln.example/',
			ADMIN_DIDS: 'did:plc:a, did:plc:b'
		});

		expect(config).toMatchObject({
			dev: false,
			publicUrl: 'https://ln.example',
			serviceDid: 'did:web:ln.example',
			allowPrivateNetwork: false,
			logLevel: 'info',
			adminDids: ['did:plc:a', 'did:plc:b']
		});
	});

	it('logs email in development, and sends it in production only through Resend with a key', () => {
		const production = { ...base, NODE_ENV: 'production', PUBLIC_URL: 'https://ln.example' };

		expect(loadConfig(base).email).toEqual({ provider: 'log', from: 'Local Network <inbox@127.0.0.1>' });
		expect(loadConfig(production).email).toEqual({ provider: 'none', from: 'Local Network <inbox@ln.example>' });
		expect(loadConfig({ ...production, RESEND_API_KEY: 're_1', EMAIL_FROM: 'LN <hi@ln.example>' }).email).toEqual({
			provider: 'resend',
			apiKey: 're_1',
			from: 'LN <hi@ln.example>'
		});
		expect(loadConfig({ ...base, EMAIL_PROVIDER: 'none', RESEND_API_KEY: 're_1' }).email.provider).toBe('none');
		expect(() => loadConfig({ ...base, EMAIL_PROVIDER: 'resend' })).toThrow(/RESEND_API_KEY/);
	});

	it('logs DMs in development, and sends them through Bluesky chat only once set', () => {
		expect(loadConfig(base).dm).toEqual({ provider: 'log', chatService: 'did:web:api.bsky.chat' });
		expect(loadConfig({ ...base, NODE_ENV: 'production' }).dm.provider).toBe('none');
		expect(loadConfig({ ...base, DM_PROVIDER: 'bluesky', CHAT_SERVICE_DID: 'did:web:chat.test' }).dm).toEqual({
			provider: 'bluesky',
			chatService: 'did:web:chat.test'
		});
	});

	it('reports every problem at once', () => {
		expect(() => loadConfig({ ADMIN_DIDS: 'alice', SYNC_POLL_INTERVAL_MS: '5' })).toThrow(
			/ROOT_IDENTIFIER[\s\S]*ROOT_PASSWORD[\s\S]*ADMIN_DIDS[\s\S]*HAPPYVIEW_URL[\s\S]*SYNC_POLL_INTERVAL_MS/
		);
	});

	it('parses booleans and integers', () => {
		const config = loadConfig({
			...base,
			SYNC_NOTIFY: 'no',
			ALLOW_PRIVATE_NETWORK: '0',
			AUDIT_MAX_ATTEMPTS: '5',
			SESSION_DAYS: '7',
			EPK_VIEW_RETENTION_DAYS: '30',
			HAPPYVIEW_API_KEY: 'hv_key'
		});

		expect(config).toMatchObject({
			syncNotify: false,
			allowPrivateNetwork: false,
			auditMaxAttempts: 5,
			sessionDays: 7,
			epkViewRetentionDays: 30,
			happyView: { apiKey: 'hv_key' }
		});
	});

	it('takes a userinput.app space URI, treating an empty one as unset', () => {
		const space = 'at://did:plc:d42l64gsczg6qh2vo243xv5l/app.userinput.space/3mabc2xyz';

		expect(loadConfig({ ...base, USERINPUT_SPACE: space }).userinputSpace).toBe(space);
		expect(loadConfig({ ...base, USERINPUT_SPACE: '' }).userinputSpace).toBeUndefined();
		expect(loadConfig(base).userinputSpace).toBeUndefined();

		for (const wrong of [
			'https://userinput.app/s/did:plc:x/3mabc',
			'at://did:plc:x/app.userinput.discussion/3mabc',
			'at://alice.test/app.userinput.space/3mabc'
		])
			expect(() => loadConfig({ ...base, USERINPUT_SPACE: wrong })).toThrow(/USERINPUT_SPACE/);
	});
});

describe('serviceDidFor', () => {
	it('maps loopback hosts to localhost and encodes ports', () => {
		expect(serviceDidFor('http://127.0.0.1:5180')).toBe('did:web:localhost%3A5180');
		expect(serviceDidFor('https://ln.example')).toBe('did:web:ln.example');
		expect(serviceDidFor('https://ln.example:8443')).toBe('did:web:ln.example%3A8443');
	});
});
