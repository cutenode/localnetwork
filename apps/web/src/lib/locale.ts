import { getLocale, type Locale } from '#lib/paraglide/runtime.js';

/**
 * The language a locale reads in, for dates, numbers and a record's `langs`: the request's on the server, the cookie's
 * in the browser. en-XA only dresses English up to test the pages, so it formats as English, and what someone types
 * under it is still English.
 */
export const languageOf = (locale: Locale = getLocale()): string => (locale === 'en-XA' ? 'en' : locale);

/** The languages of what someone writes, for a record's `langs`: their locale's language. */
export const writerLangs = (locale?: Locale): string[] => [languageOf(locale)];
