import { page, userEvent } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { ActorSuggestion } from '#lib/actors.ts';
import HandleInput from './HandleInput.svelte';

const neon: ActorSuggestion = { did: 'did:plc:neon', handle: 'neon.test', displayName: 'Neon Tide' };

const nell: ActorSuggestion = { did: 'did:plc:nell', handle: 'nell.test' };

const field = () => page.getByRole('combobox', { name: 'Your handle' });

describe('HandleInput', () => {
	it('drops an answer that a newer keystroke made stale', async () => {
		const answers = new Map<string, (found: ActorSuggestion[]) => void>();
		await render(HandleInput, {
			search: (query) => new Promise<ActorSuggestion[]>((resolve) => answers.set(query, resolve))
		});

		await userEvent.type(field(), 'ne');
		await expect.poll(() => answers.has('ne')).toBe(true);
		await userEvent.type(field(), 'l');
		await expect.poll(() => answers.has('nel')).toBe(true);
		answers.get('nel')?.([nell]);
		await expect.element(page.getByRole('option')).toHaveAccessibleName('nell.test @nell.test');
		answers.get('ne')?.([neon, nell]);
		// A macrotask, so the stale answer has landed if it was going to.
		await new Promise((resolve) => setTimeout(resolve));

		expect(page.getByRole('option').elements()).toHaveLength(1);
		await expect
			.element(page.getByRole('status'))
			.toHaveTextContent('1 account found. Use the arrow keys to pick one.');
	});
});
