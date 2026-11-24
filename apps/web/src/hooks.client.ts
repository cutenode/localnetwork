import type { ClientInit } from '@sveltejs/kit/hooks';
import { baseLocale, extractLocaleFromCookie, overwriteGetLocale } from '#lib/paraglide/runtime.js';

// Paraglide's getLocale writes the locale cookie the first time it runs, so every visitor got one on their first page
// and their pages varied on it from then on. Only picking a language (setLocale) writes it, and a visitor who hasn't
// gets the English page every signed-out visitor shares (docs/adr/0018).
export const init: ClientInit = () => overwriteGetLocale(() => extractLocaleFromCookie() ?? baseLocale);
