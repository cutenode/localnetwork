<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { PAGE_META } from '#lib/testing/page-meta.ts';
	import { demoTrack, findTool, toolCards, tools } from '#lib/tools/index.ts';
	import Layout from '../+layout.svelte';
	import ToolsPage from './+page.svelte';
	import ToolPage from './[tool]/+page.svelte';
	import GuidePage from './[tool]/docs/[page]/+page.svelte';

	/**
	 * The live Tools pages: the real layout around the real pages, fed the real tools, their captures
	 * and the demo track's waveform. The tool pages leave out their Markdown sections, which only the
	 * server renders.
	 */
	const { Story } = defineMeta({
		title: 'Patterns/Tools',
		parameters: { layout: 'fullscreen', sveltekit_experimental: { state: { page: { url: new URL('https://localnetwork.computer/tools') } } } }
	});

	const layout = { viewer: null, pendingRequests: 0, meta: PAGE_META };

	const phone = { viewport: { value: 'mobile2', isRotated: false } };

	const guides = (id: string) => (findTool(id)?.docs ?? []).map(({ slug, title }) => ({ slug, title }));

	const toolData = (id: string) => {
		const { name, kind, tagline, summary, cues, install } = findTool(id) ?? tools[0];

		return { ...layout, tool: { id, name, kind, tagline, summary, cues, install }, track: demoTrack, sections: [], guides: guides(id) };
	};

	const guide = {
		...layout,
		tool: { id: 'locallibrary', name: 'locallibrary' },
		doc: { slug: 'install', title: 'Installing locallibrary', path: 'apps/locallibrary/docs/install.md', html: '<h2 id="install">Install</h2><p>There hasn’t been a release yet, so for now you build it from source.</p>' },
		guides: guides('locallibrary')
	};
</script>

{#snippet tools_()}
	<Layout data={layout} params={{}}><ToolsPage data={{ ...layout, tools: toolCards() }} params={{}} form={null} /></Layout>
{/snippet}

{#snippet tool(id: string)}
	<Layout data={layout} params={{}}><ToolPage data={toolData(id)} params={{ tool: id }} form={null} /></Layout>
{/snippet}

{#snippet guidePage()}
	<Layout data={layout} params={{}}><GuidePage data={guide} params={{ tool: 'locallibrary', page: 'install' }} form={null} /></Layout>
{/snippet}

<Story name="Tools" asChild>{@render tools_()}</Story>

<Story name="Tools on a phone" asChild globals={phone}>{@render tools_()}</Story>

<Story name="locallibrary" asChild>{@render tool('locallibrary')}</Story>

<Story name="locallibrary on a phone" asChild globals={phone}>{@render tool('locallibrary')}</Story>

<Story name="localtoolbox" asChild>{@render tool('localtoolbox')}</Story>

<Story name="localtoolbox on a phone" asChild globals={phone}>{@render tool('localtoolbox')}</Story>

<Story name="Guide" asChild>{@render guidePage()}</Story>

<Story name="Guide on a phone, guides folded" asChild globals={phone}>{@render guidePage()}</Story>
