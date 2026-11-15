import { readdirSync } from 'node:fs';
import { expect, it } from 'vitest';
import { isNotificationKind, KINDS } from './kinds.ts';

const files = readdirSync(new URL('kinds', import.meta.url)).map((file) => file.replace(/\.ts$/, ''));

it('collects every file in kinds/ under its name, so a new kind only needs its own file', async () => {
	expect(Object.keys(KINDS).toSorted()).toEqual(files.toSorted());

	for (const name of files) {
		const file: { default: unknown } = await import(`./kinds/${name}.ts`);

		expect(isNotificationKind(name)).toBe(true);
		expect(KINDS).toHaveProperty([name], file.default);
	}
});

it('knows no kind that has no file', () => {
	expect(isNotificationKind('booking')).toBe(false);
	expect(isNotificationKind('toString')).toBe(false);
});
