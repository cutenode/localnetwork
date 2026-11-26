import { describe, expect, it } from 'vitest';
import { moves, pathRewriter } from './page-only.ts';

const LIB = 'apps/web/src/lib';

describe('moves', () => {
	it('renames the old modules still tracked, unless the new name is taken', () => {
		const tracked = [
			`${LIB}/bookings.ts`,
			`${LIB}/combobox.svelte.ts`,
			`${LIB}/combobox.page-only.svelte.ts`,
			`${LIB}/other.ts`
		];

		expect(moves(tracked)).toEqual([[`${LIB}/bookings.ts`, `${LIB}/bookings.page-only.ts`]]);
	});
});

describe('pathRewriter', () => {
	const rewrite = pathRewriter([
		`${LIB}/bookings.page-only.ts`,
		`${LIB}/form-draft.page-only.svelte.ts`,
		'apps/web/src/a.ts'
	]);

	it('rewrites imports by alias and relative path, with or without the extension', () => {
		const text = [
			"import { a } from '$lib/bookings';",
			"import { b } from '#lib/bookings.ts';",
			"import { c } from './lib/form-draft.svelte';",
			"import Table from '../src/lib/ui/ResponsiveTable.svelte';",
			"import { d } from '$lib/other';"
		].join('\n');

		expect(rewrite('apps/web/src/a.ts', text)).toBe(
			[
				"import { a } from '$lib/bookings.page-only';",
				"import { b } from '#lib/bookings.page-only.ts';",
				"import { c } from './lib/form-draft.page-only.svelte';",
				"import Table from '../src/lib/ui/ResponsiveTable.page-only.svelte';",
				"import { d } from '$lib/other';"
			].join('\n')
		);
	});

	it("rewrites a path in a doc from the app's folders or the root, and a unique bare name in apps/web", () => {
		const text = 'See src/lib/bookings.ts, apps/web/src/lib/bookings.ts and form-draft.svelte.ts.';

		expect(rewrite('apps/web/README.md', text)).toBe(
			'See src/lib/bookings.page-only.ts, apps/web/src/lib/bookings.page-only.ts and form-draft.page-only.svelte.ts.'
		);
		expect(rewrite('docs/notes.md', text)).toBe(
			'See src/lib/bookings.page-only.ts, apps/web/src/lib/bookings.page-only.ts and form-draft.svelte.ts.'
		);
	});

	it('leaves URLs, longer names, prose and other files alone', () => {
		const text = [
			'https://github.com/o/r/blob/abc/apps/web/src/lib/bookings.ts',
			'apps/web/src/lib/bookings.tsx and apps/web/src/lib/bookings.ts.map',
			"the bookings page, other/bookings.ts, 'src/lib/bookings' and //apps/web/src/lib/bookings.ts"
		].join('\n');

		expect(rewrite('apps/web/notes.md', text)).toBe(text);
		expect(rewrite('apps/web/notes.rs', 'apps/web/src/lib/bookings.ts')).toBe('apps/web/src/lib/bookings.ts');
	});

	it('leaves a bare name another tracked file has', () => {
		const shared = pathRewriter(['apps/web/src/form-draft.svelte.ts']);

		expect(shared('apps/web/README.md', 'form-draft.svelte.ts')).toBe('form-draft.svelte.ts');
	});
});
