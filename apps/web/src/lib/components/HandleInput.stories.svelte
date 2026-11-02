<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { ActorSuggestion } from '#lib/actors.ts';
	import HandleInput from './HandleInput.svelte';

	const accounts: ActorSuggestion[] = [
		{ did: 'did:plc:neon', handle: 'neon.test', displayName: 'Neon Tide' },
		{ did: 'did:plc:nell', handle: 'nell.bsky.social', displayName: 'Nell' },
		{ did: 'did:plc:nebula', handle: 'nebula-sound.bsky.social' },
		{ did: 'did:plc:ned', handle: 'ned.eurosky.social', displayName: 'Ned at the decks, every second Friday at Saltmarsh' }
	];

	/** Answers from the list above, so the stories never leave the page. */
	const search = async (query: string) => accounts.filter((actor) => actor.handle.startsWith(query) || actor.displayName?.toLowerCase().startsWith(query));

	const { Story } = defineMeta({ title: 'Patterns/Handle input', component: HandleInput, tags: ['autodocs'], args: { search } });

	const phone = { viewport: { value: 'mobile2', isRotated: false } };

	/** Types "ne" and moves to the second suggestion, the way someone picking with the keyboard would. */
	const typeAndPick = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		const input = canvasElement.querySelector<HTMLInputElement>('input[role="combobox"]');

		if (!input) return;
		input.focus();
		input.value = 'ne';
		input.dispatchEvent(new Event('input', { bubbles: true }));
		await new Promise((resolve) => setTimeout(resolve, 300));
		input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
		input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
	};
</script>

<Story name="Empty" />

<Story name="Suggestions" play={typeAndPick} />

<Story name="Unknown handle" args={{ value: 'nobody-here.test', invalid: true }} />

<Story name="Phone, suggestions" play={typeAndPick} globals={phone} />
