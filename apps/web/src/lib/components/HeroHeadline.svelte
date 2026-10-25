<script lang="ts">
	import { cubicOut } from 'svelte/easing';
	import { fly } from 'svelte/transition';
	import headline from '#lib/headline.json' with { type: 'json' };

	/**
	 * "<verb> your <noun>.", starting on the first pair, which is what the server renders and screen
	 * readers read. The verb and the noun each swap to a random other word from headline.json on their
	 * own random timer, about `interval` apart on average, sliding in from above or below at random.
	 * "your" stays put: each slot keeps its longest word's size, so nothing around it moves. With
	 * reduced motion it stays on the first pair.
	 */
	let { interval = 2200 }: { interval?: number } = $props();

	// The verb hugs "your" from the left and the noun from the right. Too narrow for one line, the
	// three stack centered instead, since a slot sized for its longest word would leave a short one
	// off to the side.
	const slots = [
		{ words: headline.verbs, align: 'text-end' },
		{ words: headline.nouns.map((noun) => `${noun}.`), align: 'text-start' }
	];

	const index = $state([0, 0]);

	// 1 when the new word drops in from above, -1 when it rises from below.
	const fall = $state([1, 1]);

	$effect(() => {
		if (matchMedia('(prefers-reduced-motion: reduce)').matches) return;

		const timers: ReturnType<typeof setTimeout>[] = [];
		// Anywhere from 0.6 to 1.4 intervals, so the two slots drift apart instead of swapping together.
		const wait = () => interval * (0.6 + Math.random() * 0.8);

		const swap = (s: number) => {
			const { length } = slots[s].words;
			// Any word but the one showing. A one-word list lands back on its word, which Svelte leaves be.
			index[s] = (index[s] + 1 + Math.floor(Math.random() * (length - 1))) % length;
			fall[s] = Math.random() < 0.5 ? 1 : -1;
			timers[s] = setTimeout(() => swap(s), wait());
		};

		slots.forEach((_, s) => (timers[s] = setTimeout(() => swap(s), wait())));

		return () => timers.forEach(clearTimeout);
	});
</script>

{#snippet slot(s: number)}
	{@const { words, align } = slots[s]}
	<!-- Every word sits hidden in the same cell, so the slot is always its longest word's size. -->
	<span class={['inline-grid max-sm:grid max-sm:text-center', align]}>
		{#each words as word (word)}<span class="invisible col-start-1 row-start-1">{word}</span>{/each}
		<!-- The old word leaves the way the new one is heading before the new one comes in, so the two never blur together. -->
		{#key index[s]}
			<span class="col-start-1 row-start-1" in:fly={{ y: `${-0.3 * fall[s]}em`, delay: 200, duration: 300, easing: cubicOut }} out:fly={{ y: `${0.3 * fall[s]}em`, duration: 200 }}>{words[index[s]]}</span>
		{/key}
	</span>
{/snippet}

<span class="sr-only">{slots[0].words[0]} your {slots[1].words[0]}</span>
<span aria-hidden="true">{@render slot(0)} <span class="max-sm:block">your</span> {@render slot(1)}</span>
