<script lang="ts">
	import headline from '#lib/headline.json' with { type: 'json' };

	/**
	 * "<verb> your <noun>.", looping through the words in headline.json from the first pair, which is
	 * what the server renders and screen readers read. Every phrase sits in the same grid cell, so the
	 * headline keeps the biggest one's size and nothing below it moves. With reduced motion it stays
	 * on the first pair.
	 */
	let { interval = 2200 }: { interval?: number } = $props();

	const { verbs, nouns } = headline;

	const phrases = Array.from({ length: Math.max(verbs.length, nouns.length) }, (_, i) => `${verbs[i % verbs.length]} your ${nouns[i % nouns.length]}.`);

	let step = $state(0);

	$effect(() => {
		if (phrases.length < 2 || matchMedia('(prefers-reduced-motion: reduce)').matches) return;

		const timer = setInterval(() => (step = (step + 1) % phrases.length), interval);

		return () => clearInterval(timer);
	});
</script>

<span class="sr-only">{phrases[0]}</span>
<!-- The old phrase fades out before the new one fades in, so the two never blur together. -->
<span aria-hidden="true" class="grid">
	{#each phrases as phrase, i (i)}
		<span class={['col-start-1 row-start-1 self-center transition-[opacity,translate,visibility] motion-reduce:transition-none', i === step ? 'opacity-100 delay-200 duration-300' : 'invisible translate-y-[0.15em] opacity-0 duration-200']}>{phrase}</span>
	{/each}
</span>
