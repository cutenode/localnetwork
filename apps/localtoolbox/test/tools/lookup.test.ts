// src/tools/artists/lookup.ts — asking a SoundCloud tab about artists.
import { describe, it, expect, beforeEach, vi } from 'vitest';
import { createChrome, type ChromeMock } from '../helpers/chrome.ts';

declare const chrome: ChromeMock;

const TAB = { id: 7, url: 'https://soundcloud.com/bicep' };

async function fresh() {
	vi.resetModules();

	return import('../../src/tools/artists/lookup.ts');
}

beforeEach(() => {
	globalThis.chrome = createChrome({ tab: TAB });
});

describe('whether the tab in front can be asked', () => {
	it('is yes for any SoundCloud page, and no for anything else', async () => {
		const { onSoundCloud } = await fresh();
		expect(await onSoundCloud()).toBe(true);

		for (const tab of [null, { id: 1, url: 'https://example.com/' }, { id: 2 }]) {
			globalThis.chrome = createChrome({ tab });
			const lookup = await fresh();
			expect(await lookup.onSoundCloud(), String(tab?.url)).toBe(false);
		}
	});
});

describe('the lookup', () => {
	it('asks the page for the profiles, naming the artists', async () => {
		const { lookupProfiles } = await fresh();
		const answer = { ok: true, profiles: [{ permalink: '/bicep' }], failed: [], error: '' };
		chrome.tabs.sendMessage.mockResolvedValue(answer);
		expect(await lookupProfiles(['/bicep'])).toEqual({ data: answer });
		expect(chrome.tabs.sendMessage).toHaveBeenCalledWith(7, {
			type: 'GET_ARTIST_PROFILES',
			permalinks: ['/bicep']
		});
	});

	it('asks for a SoundCloud tab when there is none in front', async () => {
		globalThis.chrome = createChrome({ tab: { id: 1, url: 'https://example.com/' } });
		const { lookupProfiles, OPEN_A_TAB } = await fresh();
		expect(await lookupProfiles(['/bicep'])).toEqual({ status: OPEN_A_TAB });
		expect(chrome.tabs.sendMessage).not.toHaveBeenCalled();
	});

	it('says to reload a tab the message cannot reach', async () => {
		const { lookupProfiles } = await fresh();
		chrome.tabs.sendMessage.mockRejectedValue(new Error('Could not establish connection'));
		expect((await lookupProfiles(['/bicep'])).status).toMatch(/can’t reach the page/);
	});

	it('says the lookup failed for an answer it cannot read', async () => {
		const { lookupProfiles, LOOKUP_FAILED } = await fresh();

		for (const response of [undefined, null, { ok: false, error: 'boom' }, { ok: true }, { ok: true, profiles: 'x' }]) {
			chrome.tabs.sendMessage.mockResolvedValue(response);
			expect(await lookupProfiles(['/bicep']), JSON.stringify(response)).toEqual({
				status: LOOKUP_FAILED
			});
		}
	});

	it('has words for what the content script says went wrong', async () => {
		const { LOOKUP_ERRORS } = await fresh();
		expect(LOOKUP_ERRORS['no-client-id']).toMatch(/API key/);
		expect(LOOKUP_ERRORS.unauthorized).toMatch(/rejected/);
		expect(LOOKUP_ERRORS['']).toBeUndefined();
	});
});
