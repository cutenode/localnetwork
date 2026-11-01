<script lang="ts">
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

	const cells: HTMLElement[] = [];

	// The animations are played by hand rather than with svelte/transition, whose runtime alone would
	// put the client bundle over its budget (tests/build.test.ts).
	const shift = (by: number) => `translateY(${by * 0.3}em)`;

	$effect(() => {
		if (matchMedia('(prefers-reduced-motion: reduce)').matches) return;

		const timers: ReturnType<typeof setTimeout>[] = [];
		// Anywhere from 0.6 to 1.4 intervals, so the two slots drift apart instead of swapping together.
		const wait = () => interval * (0.6 + Math.random() * 0.8);

		const swap = (s: number) => {
			const { length } = slots[s].words;
			// Any word but the one showing.
			const next = (index[s] + 1 + Math.floor(Math.random() * (length - 1))) % length;
			// 1 drops the new word in from above, -1 raises it from below.
			const fall = Math.random() < 0.5 ? 1 : -1;

			// The old word leaves the way the new one is heading before the new one comes in, so the two
			// never blur together.
			cells[s].children[index[s]].animate([{ visibility: 'visible' }, { visibility: 'visible', opacity: 0, transform: shift(fall) }], { duration: 200 });
			cells[s].children[next].animate([{ opacity: 0, transform: shift(-fall) }, { opacity: 1, transform: 'none' }], { delay: 200, duration: 300, easing: 'cubic-bezier(0.33, 1, 0.68, 1)', fill: 'backwards' });
			index[s] = next;
			timers[s] = setTimeout(() => swap(s), wait());
		};

		slots.forEach((slot, s) => {
			if (slot.words.length > 1) timers[s] = setTimeout(() => swap(s), wait());
		});

		return () => timers.forEach(clearTimeout);
	});
</script>

{#snippet slot(s: number)}
	{@const { words, align } = slots[s]}
	<!-- Every word sits in the same cell, so the slot is always its longest word's size. -->
	<span bind:this={cells[s]} class={['inline-grid max-sm:grid max-sm:text-center', align]}>
		{#each words as word, i (word)}<span class={['col-start-1 row-start-1', i !== index[s] && 'invisible']}>{word}</span>{/each}
	</span>
{/snippet}

<span class="sr-only">{slots[0].words[0]} your {slots[1].words[0]}</span>
<span aria-hidden="true">{@render slot(0)} <span class="max-sm:block">your</span> {@render slot(1)}</span>
