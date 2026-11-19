<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect } from 'storybook/test';
	import type { PageMeta } from '#lib/meta.ts';
	import { PAGE_META } from '#lib/testing/page-meta.ts';
	import CodeBlock from '#lib/ui/CodeBlock.svelte';
	import SocialMeta from './SocialMeta.svelte';

	// No autodocs: every story on one docs page would write into the same head, and each preview would show them all.
	const { Story } = defineMeta({ title: 'Patterns/Social meta', component: SocialMeta });

	/** The tags SocialMeta writes; the title is read off the document, since Svelte sets it rather than adding one. */
	const WRITTEN = 'meta[name="description"], link[rel="canonical"], meta[name="robots"], meta[property^="og:"], meta[name^="twitter:"]';

	const project: PageMeta = {
		...PAGE_META,
		title: 'Neon Tide',
		description: 'Rolling techno from Berlin.',
		url: 'https://localnetwork.test/projects/neon-tide',
		type: 'profile',
		image: { url: 'https://localnetwork.test/og/project/neon-tide/bafy.png', alt: 'Neon Tide, a solo DJ from Berlin', width: 1200, height: 630 }
	};

	const profile: PageMeta = { ...project, url: 'https://localnetwork.test/@neon.test', image: { url: 'https://localnetwork.test/media/did:plc:neon/bafyavatar', alt: 'Neon Tide' } };

	const home: PageMeta = {
		...PAGE_META,
		title: 'Local Network',
		documentTitle: 'Local Network · Find local DJs for your show',
		description: 'Promoters find local DJs.',
		noindex: true
	};

	/** Checks the preview shows `tag`, so a story can't pass with an empty one. */
	const shows =
		(tag: string) =>
		async ({ canvasElement }: { canvasElement: HTMLElement }) => {
			await expect(canvasElement.textContent).toContain(tag);
		};
</script>

<script lang="ts">
	let head = $state('');

	/** Reads back what SocialMeta wrote, once it has mounted, since the head isn't on the canvas. */
	const readHead = () => {
		const tags = [...document.head.querySelectorAll(WRITTEN)].map((tag) => tag.outerHTML);

		head = [`<title>${document.title}</title>`, ...tags].join('\n');
	};
</script>

{#snippet preview(meta: PageMeta)}
	<SocialMeta {meta} />
	<div {@attach readHead}><CodeBlock code={head} title="<head>" meta="what crawlers and link previews read" /></div>
{/snippet}

<Story name="Wide card" asChild play={shows('<meta name="twitter:card" content="summary_large_image">')}>{@render preview(project)}</Story>

<Story name="Square avatar" asChild play={shows('<meta name="twitter:card" content="summary">')}>{@render preview(profile)}</Story>

<Story name="Kept out of search" asChild play={shows('<meta name="robots" content="noindex">')}>{@render preview(home)}</Story>
