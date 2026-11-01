<script lang="ts">
	import headline from '#lib/headline.json' with { type: 'json' };
	import { ink } from '../style.ts';
	import CardFrame from './CardFrame.svelte';

	/** The link preview of the site's own pages: the homepage's headline, as the server renders it, and its lede. */
	const words = `${headline.verbs[0]} your ${headline.nouns[0]}.`;
</script>

<CardFrame>
	<div style="display: flex; flex-direction: column; justify-content: center; flex-grow: 1;">
		<div style="display: flex; font-size: 136px; font-weight: 600; line-height: 1; letter-spacing: -0.045em;">{words}</div>
		<div style="display: flex; max-width: 900px; margin-top: 28px; font-size: 36px; line-height: 1.35; color: {ink.fgMuted};">Promoters find DJs by sound, distance and budget. DJs keep a profile they own.</div>
	</div>
</CardFrame>
