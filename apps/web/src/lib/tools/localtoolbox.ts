import artists from './captures/localtoolbox/artists.png?enhanced&w=2560;1600;960';
import artistsPhone from './captures/localtoolbox/artists-phone.png?enhanced';
import manager from './captures/localtoolbox/manager.png?enhanced&w=2560;1600;960';
import managerPhone from './captures/localtoolbox/manager-phone.png?enhanced';
import marks from './captures/localtoolbox/marks.png?enhanced&w=2560;1600;960';
import marksPhone from './captures/localtoolbox/marks-phone.png?enhanced';
import menu from './captures/localtoolbox/menu.png?enhanced&w=2560;1600;960';
import menuPhone from './captures/localtoolbox/menu-phone.png?enhanced';
import playlistToText from './captures/localtoolbox/playlist-to-text.png?enhanced&w=2560;1600;960';
import playlistToTextPhone from './captures/localtoolbox/playlist-to-text-phone.png?enhanced';
import { loadDocs } from './docs.ts';
import type { Tool } from './types.ts';

export const localtoolbox: Tool = {
	id: 'localtoolbox',
	name: 'localtoolbox',
	kind: 'Chrome extension',
	tagline: 'Track lists, download links, artists and a queue, right on SoundCloud.',
	summary:
		'Open it from the toolbar while you browse SoundCloud and pick a tool. Tracks you pick gather in a session queue, so a whole batch can be marked, downloaded or handed on at once.',
	cues: [
		{
			name: 'Marks',
			line: "Marks the tracks you've downloaded right on SoundCloud's pages",
			capture: {
				picture: marks,
				phone: marksPhone,
				alt: "A SoundCloud playlist with localtoolbox's marks on it: a tick on the cover art of each track already downloaded, and a nodl pill on the one with no download"
			}
		},
		{
			name: 'Downloads',
			line: 'Shows where each track can be downloaded, and works through gates like Hypeddit, ToneDen and Droploud',
			capture: {
				picture: manager,
				phone: managerPhone,
				alt: "localtoolbox's manager, open on a SoundCloud playlist: its tracks grouped by the site each one downloads from, with buttons to download, queue or mark each"
			}
		},
		{
			name: 'Text',
			line: 'Turns a playlist, Likes page or track page into plain text, CSV, TSV or JSON',
			capture: {
				picture: playlistToText,
				phone: playlistToTextPhone,
				alt: 'localtoolbox turning a SoundCloud playlist into ten lines of artist and title, ready to copy or download'
			}
		},
		{
			name: 'Artists',
			line: 'Keeps a list of artists with their socials and contact addresses',
			capture: {
				picture: artists,
				phone: artistsPhone,
				alt: "localtoolbox's artist list: each artist with their city, socials and contact address"
			}
		},
		{
			name: 'Tools',
			line: 'Opens from the toolbar on any SoundCloud page, and saves everything to one JSON file',
			capture: { picture: menu, phone: menuPhone, alt: "localtoolbox's menu, listing each of its tools" }
		}
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
	status: 'Free. Not in the Chrome Web Store yet: you build it and load it into Chrome.',
	// Its README is the developers' spec, so the guides are docs/*.md, led by docs/overview.md.
	docs: loadDocs(
		'apps/localtoolbox',
		import.meta.glob<string>('../../../../localtoolbox/docs/*.md', {
			query: '?raw',
			import: 'default',
			eager: true
		})
	)
};
