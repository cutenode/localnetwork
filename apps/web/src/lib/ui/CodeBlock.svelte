<script lang="ts">
	/**
	 * Code, a record or a terminal session. `json` colours keys; `shell` dims the `$` prompt and
	 * colours lines that start with ✓ or !. Without `title` or `meta` there's no caption, and
	 * `framed={false}` drops the border for a block that sits inside a card.
	 */
	let {
		code,
		language = 'text',
		title,
		meta,
		framed = true
	}: { code: string; language?: 'text' | 'json' | 'shell'; title?: string; meta?: string; framed?: boolean } = $props();

	type Token = { text: string; tone?: string };

	/** What a shell line's first character says about it: a prompt, a pass or a warning. */
	const marks = { $: 'text-fg-subtle', '✓': 'text-good', '!': 'text-warning' } as const;

	const isMark = (c: string): c is keyof typeof marks => Object.hasOwn(marks, c);

	const tokenize = (line: string): Token[] => {
		if (language === 'json') return line.split(/("[^"]*")(?=\s*:)/).map((text, i) => (i % 2 ? { text, tone: 'text-link' } : { text }));

		const head = line.charAt(0);

		if (language === 'shell' && isMark(head)) return [{ text: head, tone: marks[head] }, { text: line.slice(1) }];

		return [{ text: line }];
	};

	const lines = $derived(code.split('\n').map(tokenize));
</script>

{#snippet block()}
	<!-- Focusable so a keyboard can scroll a long line into view. -->
	<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
	<pre tabindex="0" class="m-0 overflow-x-auto bg-canvas p-4 font-mono text-label leading-[1.75] text-fg-soft"><code
			>{#each lines as line, i (i)}<span class="block min-h-[1lh]">{#each line as token, j (j)}{#if token.tone}<span class={token.tone}>{token.text}</span>{:else}{token.text}{/if}{/each}</span>{/each}</code
		></pre>
{/snippet}

{#if framed}
	<figure class="m-0 min-w-0 overflow-hidden rounded-xl border border-line bg-surface">
		{#if title || meta}
			<figcaption class="flex justify-between gap-4 border-b border-line px-3.5 py-2.5 font-mono text-caption text-fg-subtle">
				<span>{title}</span>
				{#if meta}<span>{meta}</span>{/if}
			</figcaption>
		{/if}
		{@render block()}
	</figure>
{:else}
	{@render block()}
{/if}
