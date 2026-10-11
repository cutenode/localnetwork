import { loadDocs } from './docs.ts';
import type { Tool } from './types.ts';

export const localtoolbox: Tool = {
	id: 'localtoolbox',
	name: 'localtoolbox',
	kind: 'Chrome extension',
	tagline: 'Track lists, download links, artists and a queue, right on SoundCloud.',
	summary:
		'Open it from the toolbar while you browse SoundCloud and pick a tool. Tracks you pick gather in a session queue, so a whole batch can be marked, downloaded or handed on at once.',
	features: [
		'Turns a playlist, Likes page or track page into plain text, CSV, TSV or JSON',
		'Keeps a list of tracks and shows where each one can be downloaded',
		"Marks the tracks you've downloaded right on SoundCloud's pages",
		'Works through download gates like Hypeddit, ToneDen and Droploud and saves what comes back',
		'Keeps a list of artists with their socials and contact addresses',
		'Saves everything to one JSON file and reads it back'
	],
	install: {
		note: "It isn't in the Chrome Web Store yet, so for now you build it and load it into Chrome yourself.",
		steps: [
			{
				text: 'With Node.js 24 and pnpm installed, download it and build it.',
				command:
					'git clone https://github.com/cutenode/localnetwork.git\ncd localnetwork\npnpm install\npnpm -C apps/localtoolbox build'
			},
			{ text: 'In Chrome, open chrome://extensions and turn on Developer mode, top right.' },
			{ text: 'Click Load unpacked and pick the apps/localtoolbox/dist folder.' }
		]
	},
	docs: loadDocs(
		'apps/localtoolbox',
		import.meta.glob<string>('../../../../localtoolbox/{README.md,docs/*.md}', {
			query: '?raw',
			import: 'default',
			eager: true
		})
	)
};
