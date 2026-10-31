<script lang="ts">
	import type { Snippet } from 'svelte';

	/** One card of a long form: a titled fieldset and a line on what it's for. */
	let { title, description, children }: { title: string; description: string; children: Snippet } = $props();
</script>

<!-- min-w-0: a fieldset is otherwise as wide as its widest content, which can outgrow a phone. -->
<fieldset class="card flex min-w-0 flex-col gap-5 p-5 sm:p-6">
	<!-- Floated, a legend leaves the fieldset's border alone and lays out like the other flex items. -->
	<legend class="eyebrow float-left -mb-3.5 text-fg-subtle">{title}</legend>
	<p class="text-small text-fg-muted">{description}</p>
	{@render children()}
</fieldset>
