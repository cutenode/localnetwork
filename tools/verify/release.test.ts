import { describe, expect, it } from 'vitest';
import { releaseProblems } from './release.ts';

const plan = (...apps: string[]) => JSON.stringify({ releases: apps.map((app_name) => ({ app_name })) });

describe('releaseProblems', () => {
	it('passes a plan that releases locallibrary-cli alone', () => {
		expect(releaseProblems(plan('locallibrary-cli'))).toEqual([]);
	});

	it('names what else dist would release', () => {
		expect(releaseProblems(plan('locallibrary-cli', 'loclib-new'))).toEqual([
			expect.stringMatching(
				/^dist would release locallibrary-cli, loclib-new, not only locallibrary-cli: set dist = false/
			)
		]);
	});

	it('fails a plan that releases nothing', () => {
		expect(releaseProblems('{}')).toEqual([expect.stringMatching(/^dist would release nothing, not only/)]);
	});
});
