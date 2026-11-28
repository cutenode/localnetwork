<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { colors } from '#lib/mix/report.ts';

	const { Story } = defineMeta({ title: 'Foundations' });

	const surfaces = [
		['canvas', 'bg-canvas', 'The page'],
		['sunken', 'bg-sunken', 'Every other section'],
		['surface', 'bg-surface', 'Cards and panels'],
		['raised', 'bg-raised', 'Secondary buttons, pills'],
		['well', 'bg-well', 'Inputs, insets, the grid'],
		['line', 'bg-line', 'Borders'],
		['line-strong', 'bg-line-strong', 'Control borders']
	] as const;

	const inks = [
		['fg', 'text-fg', 'Headings and body'],
		['fg-soft', 'text-fg-soft', 'Secondary body'],
		['fg-muted', 'text-fg-muted', 'Ledes and descriptions'],
		['fg-subtle', 'text-fg-subtle', 'Labels and metadata'],
		['link', 'text-link', 'Links and focus']
	] as const;

	const data = [
		['Waveform bands', colors.bands.low, colors.bands.mid, colors.bands.high],
		['Phrases', ...Object.values(colors.phrases)],
		['Track colours', ...Object.values(colors.trackColors)],
		['Cues', ...Object.values(colors.cues)],
		['Status', ...Object.values(colors.status.dark)]
	];

	const type = [
		['display', 'text-display', 'Hero, once a page'],
		['title', 'text-title', 'Section titles'],
		['heading', 'text-heading', 'Card and panel titles'],
		['intro', 'text-intro text-fg-muted', 'Hero lede'],
		['lede', 'text-lede text-fg-muted', 'Section lede'],
		['emphasis', 'text-emphasis', 'Names in lists'],
		['body', 'text-body', 'Running text'],
		['small', 'text-small text-fg-muted', 'Descriptions under a name'],
		['label', 'text-label', 'Chips, controls, captions'],
		['caption', 'font-mono text-caption text-fg-subtle', 'Mono: numbers, record names'],
		['micro', 'font-mono text-micro text-fg-subtle', 'Badges and monograms only']
	] as const;

	const radii = ['xs', 'sm', 'md', 'lg', 'xl', '2xl', 'full'] as const;

	const radiusClass = { xs: 'rounded-xs', sm: 'rounded-sm', md: 'rounded-md', lg: 'rounded-lg', xl: 'rounded-xl', '2xl': 'rounded-2xl', full: 'rounded-full' };
</script>

<Story name="Colour" asChild>
	<div class="flex max-w-page flex-col gap-10">
		<section aria-labelledby="f-surfaces" class="flex flex-col gap-3">
			<h2 id="f-surfaces" class="text-heading">Surfaces</h2>
			<ul class="grid grid-cols-[repeat(auto-fill,minmax(10rem,1fr))] gap-3">
				{#each surfaces as [name, swatch, use] (name)}
					<li class="flex flex-col gap-2"><span class="h-16 rounded-lg border border-line-strong {swatch}"></span><span class="font-mono text-caption">{name}</span><span class="text-label text-fg-muted">{use}</span></li>
				{/each}
			</ul>
		</section>
		<section aria-labelledby="f-ink" class="flex flex-col gap-3">
			<h2 id="f-ink" class="text-heading">Ink</h2>
			<ul class="flex flex-col gap-2">
				{#each inks as [name, ink, use] (name)}
					<li class="flex flex-wrap items-baseline gap-x-4"><span class="w-32 text-emphasis {ink}">Neon Tide</span><span class="font-mono text-caption">{name}</span><span class="text-label text-fg-muted">{use}</span></li>
				{/each}
			</ul>
		</section>
		<section aria-labelledby="f-data" class="flex flex-col gap-3">
			<h2 id="f-data" class="text-heading">DJ data</h2>
			<p class="max-w-160 text-fg-muted">The only colour in the system. Generated from packages/tokens/colors.json, so the site, the player and the HTML report agree.</p>
			{#each data as [name, ...swatches] (name)}
				<div class="flex flex-wrap items-center gap-3">
					<span class="w-36 text-label">{name}</span>
					{#each swatches as hex, i (i)}<span title={hex} class="size-8 rounded-md" style:background-color={hex}></span>{/each}
				</div>
			{/each}
		</section>
	</div>
</Story>

<Story name="Type" asChild>
	<dl class="grid max-w-page grid-cols-[8rem_minmax(0,1fr)] items-baseline gap-x-6 gap-y-5">
		{#each type as [name, style, use] (name)}
			<dt class="flex flex-col"><span class="font-mono text-caption">{name}</span><span class="text-label text-fg-subtle">{use}</span></dt>
			<dd class="min-w-0 {style}">Book the right DJ from your own scene.</dd>
		{/each}
	</dl>
</Story>

<Story name="Shape" asChild>
	<div class="flex flex-col gap-6">
		<ul class="flex flex-wrap gap-4">
			{#each radii as radius (radius)}
				<li class="flex flex-col items-center gap-2"><span class="size-20 border border-line-strong bg-surface {radiusClass[radius]}"></span><span class="font-mono text-caption">{radius}</span></li>
			{/each}
		</ul>
		<div class="h-40 max-w-page rounded-xl border border-line bg-grid"></div>
		<p class="text-label text-fg-muted">bg-grid, the 64px grid behind the hero and the closing call.</p>
	</div>
</Story>
