<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import ScoreBar from './ScoreBar.svelte';

	const { Story } = defineMeta({ title: 'Primitives/Score bar', component: ScoreBar, tags: ['autodocs'], args: { score: 72 } });
</script>

<Story name="Score" />

<Story name="Range" asChild>
	<!-- Scores past either end fill the bar to its edge but still print as given. -->
	<div class="flex flex-col gap-2">{#each [0, 18, 50, 87, 100, 120] as score (score)}<ScoreBar {score} />{/each}</div>
</Story>
