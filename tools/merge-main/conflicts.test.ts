import { describe, expect, it } from 'vitest';
import { renameOnly } from './conflicts.ts';

describe('renameOnly', () => {
	it("holds when main's side only adds prop_ to names and .page-only to module paths", () => {
		const base =
			"fn keeps_order() {}\nuse crate::r#keeps;\nimport { a } from '$lib/bookings.ts';\nimport { b } from '$lib/bookings';\n";

		const theirs =
			"fn prop_keeps_order() {}\nuse crate::r#prop_keeps;\nimport { a } from '$lib/bookings.page-only.ts';\nimport { b } from '$lib/bookings.page-only';\n";

		expect(renameOnly(base, theirs)).toBe(true);
	});

	it('fails for any other change', () => {
		expect(renameOnly('fn keeps_order() {}\n', 'fn prop_keeps_order() { check(); }\n')).toBe(false);
		expect(renameOnly('fn test_order() {}\n', 'fn test_prop_order() {}\n')).toBe(false);
		expect(renameOnly("'$lib/bookings'", "'$lib/bookings.page-only-v2'")).toBe(false);
	});
});
