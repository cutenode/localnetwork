import { afterEach, beforeEach, expect, it } from 'vitest';
import { m } from '#lib/paraglide/messages.js';
import { cookieName, setLocale } from '#lib/paraglide/runtime.js';
import { init } from './hooks.client.ts';

const localeCookie = () => document.cookie.split('; ').find((cookie) => cookie.startsWith(`${cookieName}=`));

const clearLocaleCookie = () => {
	document.cookie = `${cookieName}=; Max-Age=0; Path=/`;
};

beforeEach(async () => {
	clearLocaleCookie();
	await init();
});

afterEach(clearLocaleCookie);

it("shows a first visit's page in English without writing the locale cookie", () => {
	expect(m.footer_privacy()).toBe('Privacy');
	expect(localeCookie()).toBeUndefined();
});

it('writes the locale cookie when someone picks a language', async () => {
	await setLocale('en-XA', { reload: false });

	expect(localeCookie()).toBe(`${cookieName}=en-XA`);
	expect(m.footer_privacy()).toBe('[Ƥřīṽȧȧƈẏ]');
});
