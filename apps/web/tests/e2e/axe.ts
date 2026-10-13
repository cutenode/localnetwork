import { AxeBuilder } from '@axe-core/playwright';
import { expect, type Page } from '@playwright/test';

/** Fails on any WCAG 2.1 A or AA problem axe finds in the page as it is now. */
export async function expectAccessible(page: Page) {
	const { violations } = await new AxeBuilder({ page })
		.withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
		.analyze();

	expect(
		violations.map(
			({ id, help, nodes }) => `${id}: ${help} (${nodes.map((node) => node.target.join(' ')).join(', ')})`
		),
		`axe on ${page.url()}`
	).toEqual([]);
}
