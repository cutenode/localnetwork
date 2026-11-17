import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import matrix from '../../../../docs/link-matrix.md?raw';
import { matrixRows, type LinkChecks } from '#lib/testing/link-matrix.ts';

// One module per page type, so a page type's issue adds its checks without touching the others'.
const pageTypes = Object.values(
	import.meta.glob<LinkChecks>('/src/lib/testing/link-matrix/*.ts', { eager: true, import: 'default' })
);

describe.each(pageTypes)('$pageType', ({ render, links }) => {
	it.each(Object.entries(links))('links to %s', async (row, expected) => {
		await render(row);

		for (const { name, href, within, reveal } of expected) {
			const scope = within ? page.getByRole('region', { name: within }) : page;

			if (reveal) await scope.getByText(reveal).first().click();
			await expect.element(scope.getByRole('link', { name }).first()).toHaveAttribute('href', href);
		}
	});
});

it('checks every row docs/link-matrix.md marks on main, and only those', () => {
	const onMain = matrixRows(matrix)
		.filter((row) => row.onMain === 'on main')
		.map((row) => `${row.pageType}: ${row.linksTo}`);

	const checked = pageTypes.flatMap(({ pageType, links }) => Object.keys(links).map((row) => `${pageType}: ${row}`));

	expect(checked.toSorted()).toEqual(onMain.toSorted());
});
