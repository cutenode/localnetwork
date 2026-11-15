import { readdirSync } from 'node:fs';
import { expect, it } from 'vitest';
import { KIND_FILES as ON_DISK } from '../../../../scripts/lib/kind-files.ts';
import { KIND_FILES } from './kind-files.ts';
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

it("reads the same kinds outside Vite, where the dev network's scripts have no import.meta.glob", () => {
	expect(Object.keys(ON_DISK).toSorted()).toEqual(Object.keys(KIND_FILES).toSorted());

	for (const [path, kind] of Object.entries(KIND_FILES)) expect(ON_DISK[path]).toBe(kind);
});
