import { expect, it } from 'vitest';

// vite.config.ts pins every test's locale and time zone, so a date or number reads the same on any machine.
it("runs in en-US and UTC, whatever the machine's", () => {
	const { locale, timeZone } = Intl.DateTimeFormat().resolvedOptions();

	expect({ locale, timeZone }).toEqual({ locale: 'en-US', timeZone: 'UTC' });
});
