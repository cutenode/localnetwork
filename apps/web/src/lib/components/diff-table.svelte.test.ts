import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import DiffTable from './DiffTable.svelte';

describe('DiffTable', () => {
	it('marks a removed value as none after the change', async () => {
		await render(DiffTable, { changes: [{ field: 'profile.links[0].url', before: 'https://neon.example' }] });

		expect(
			page
				.getByRole('row')
				.last()
				.getByRole('cell')
				.elements()
				.map((cell) => cell.textContent)
		).toEqual(['links #0 › url', 'https://neon.example', 'none']);
	});
});
