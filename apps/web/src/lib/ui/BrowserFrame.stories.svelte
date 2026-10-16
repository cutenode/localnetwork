<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import BrowserFrame from './BrowserFrame.svelte';

	const { Story } = defineMeta({ title: 'Primitives/Browser frame', component: BrowserFrame, tags: ['autodocs'] });
</script>

<Story name="Framed" asChild>
	<div class="max-w-180">
		<BrowserFrame url="localnetwork.computer/projects/neon-tide">
			<p class="m-0 p-7 text-fg-muted">A product screen goes here.</p>
		</BrowserFrame>
	</div>
</Story>

<Story name="Bleeding off the section" asChild>
	<div class="max-w-180 overflow-hidden pt-4">
		<BrowserFrame url="localnetwork.computer" bleed>
			<p class="m-0 p-7 text-fg-muted">The hero window runs into the next section.</p>
		</BrowserFrame>
	</div>
</Story>
