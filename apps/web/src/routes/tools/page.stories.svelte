<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { PAGE_META } from '#lib/testing/page-meta.ts';
	import { cuePoints } from '#lib/tools/cues.ts';
	import { demoTrack, findTool, tools } from '#lib/tools/index.ts';
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

	// As the page's load places them: every tool's cues along the demo track.
	const deckTrack = { ...demoTrack, cues: cuePoints(tools.flatMap((tool) => tool.cues).length, demoTrack.durationSecs, demoTrack.phrases) };

	const guides = (id: string) => (findTool(id)?.docs ?? []).map(({ slug, title }) => ({ slug, title }));

	const toolData = (id: string) => {
		const { name, kind, tagline, summary, cues, install, status } = findTool(id) ?? tools[0];

		return { ...layout, tool: { id, name, kind, tagline, summary, cues, install, status }, track: demoTrack, sections: [], guides: guides(id) };
	};

	// A guide long enough to open on its sections, with a line under each.
	const contents = [
		{ id: 'the-managers-list', title: "The Manager's list" },
		{ id: 'where-each-track-downloads', title: 'Where each track downloads' },
		{ id: 'the-session-queue', title: 'The session queue' },
		{ id: 'download-gates', title: 'Download gates' },
		{ id: 'when-a-gate-needs-you', title: 'When a gate needs you' }
	];

	const guide = {
		...layout,
		tool: { id: 'localtoolbox', name: 'localtoolbox' },
		doc: {
			slug: 'downloads',
			title: 'Downloads and gates',
			path: 'apps/localtoolbox/docs/downloads.md',
			html: contents.map(({ id, title }) => `<h2 id="${id}">${title}</h2><p>Nothing goes on the list by itself. Open a SoundCloud playlist, Likes page or track page, scroll a long list to the end, then open the Manager.</p>`).join('')
		},
		contents,
		guides: guides('localtoolbox')
	};
</script>

{#snippet tools_()}
	<Layout data={layout} params={{}}><ToolsPage data={{ ...layout, tools, track: deckTrack }} params={{}} form={null} /></Layout>
{/snippet}

{#snippet tool(id: string)}
	<Layout data={layout} params={{}}><ToolPage data={toolData(id)} params={{ tool: id }} form={null} /></Layout>
{/snippet}

{#snippet guidePage()}
	<Layout data={layout} params={{}}><GuidePage data={guide} params={{ tool: 'localtoolbox', page: 'downloads' }} form={null} /></Layout>
{/snippet}

<Story name="Tools" asChild>{@render tools_()}</Story>

<Story name="Tools on a phone" asChild globals={phone}>{@render tools_()}</Story>

<Story name="locallibrary" asChild>{@render tool('locallibrary')}</Story>

<Story name="locallibrary on a phone" asChild globals={phone}>{@render tool('locallibrary')}</Story>

<Story name="localtoolbox" asChild>{@render tool('localtoolbox')}</Story>

<Story name="localtoolbox on a phone" asChild globals={phone}>{@render tool('localtoolbox')}</Story>

<Story name="Guide" asChild>{@render guidePage()}</Story>

<Story name="Guide on a phone, guides folded" asChild globals={phone}>{@render guidePage()}</Story>
