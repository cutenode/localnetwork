import check from './captures/locallibrary/check.png?enhanced&w=2560;1600;960';
import checkPhone from './captures/locallibrary/check-phone.png?enhanced';
import drives from './captures/locallibrary/drives.png?enhanced&w=2560;1600;960';
import drivesPhone from './captures/locallibrary/drives-phone.png?enhanced';
import library from './captures/locallibrary/library.png?enhanced&w=2560;1600;960';
import libraryPhone from './captures/locallibrary/library-phone.png?enhanced';
import mixes from './captures/locallibrary/mixes.png?enhanced&w=2560;1600;960';
import mixesPhone from './captures/locallibrary/mixes-phone.png?enhanced';
import player from './captures/locallibrary/player.png?enhanced&w=2560;1600;960';
import playerPhone from './captures/locallibrary/player-phone.png?enhanced';
import report from './captures/locallibrary/report.png?enhanced&w=2560;1600;960';
import reportPhone from './captures/locallibrary/report-phone.png?enhanced';
import snapshots from './captures/locallibrary/snapshots.png?enhanced&w=2560;1600;960';
import snapshotsPhone from './captures/locallibrary/snapshots-phone.png?enhanced';
import { loadDocs } from './docs.ts';
// Written by locallibrary, which signs in as https://<host>/oauth-client-metadata.json:
//   locallibrary account client-metadata -o apps/web/src/lib/tools/locallibrary/oauth-client-metadata.json
import clientMetadata from './locallibrary/oauth-client-metadata.json?raw';
import type { Tool } from './types.ts';

export const locallibrary: Tool = {
	id: 'locallibrary',
	name: 'locallibrary',
	kind: 'Terminal app for Mac',
	tagline: 'Keeps your rekordbox USB drives in sync, backed up and edited.',
	summary:
		'A command line tool and terminal player for DJs who export from rekordbox to a stick and play it on CDJs or XDJs. It runs on a Mac (Linux and Windows are experimental). Every change shows its plan and asks first, and undo takes the last one back.',
	cues: [
		{
			name: 'Play',
			line: "Plays your drives with rekordbox's own waveforms, phrases and hot cues",
			capture: {
				picture: player,
				phone: playerPhone,
				alt: 'locallibrary playing a track in a terminal: its rekordbox waveform scrolling past the playhead, with BPM, key, the current phrase and the phrases ahead'
			}
		},
		{
			name: 'Sync',
			line: 'Keeps every copy of your library identical to the drive you export to, My Settings included',
			capture: {
				picture: drives,
				phone: drivesPhone,
				alt: "locallibrary's Drives view: the DEMO and DJ drives, and for DJ its tracks, free space, My Settings files and a store that's up to date"
			}
		},
		{
			name: 'Edit',
			line: 'Edits playlists, tags, cues, ratings and cover art right on your drives',
			capture: {
				picture: library,
				phone: libraryPhone,
				alt: "locallibrary's Library view: two drives, their playlists, and twelve tracks with artist, BPM, key and length"
			}
		},
		{
			name: 'Check',
			line: "Checks drives before a set, down to the tracks a player won't load",
			capture: {
				picture: check,
				phone: checkPhone,
				alt: 'locallibrary check on a drive for CDJ-3000 and CDJ-2000NXS2 players: ready, with a warning that twelve tracks have no waveform, beat grid or cues'
			}
		},
		{
			name: 'History',
			line: 'Keeps a history of your library, so a lost drive can be rebuilt',
			capture: {
				picture: snapshots,
				phone: snapshotsPhone,
				alt: "locallibrary's Snapshots view: the library's saved versions over time, any of which can go back on a drive"
			}
		},
		{
			name: 'Tracklist',
			line: 'Turns the history players record into tracklists for Mixcloud, YouTube or a CUE sheet',
			capture: {
				picture: mixes,
				phone: mixesPhone,
				alt: "A recorded set in locallibrary's Mixes view: the set's waveform, four tracks identified along a timeline, and the key and tempo of each mix"
			}
		},
		{
			name: 'Report',
			line: 'Makes a report of a recorded set to share, with its waveform, tracklist and tempo',
			capture: {
				picture: report,
				phone: reportPhone,
				alt: "A recorded set's report in a browser: the mix's waveform with each track along a timeline, the tracklist and a tempo chart"
			}
		}
	],
	install: {
		note: "There's no download yet. For now you build it from source with Rust, which takes a few minutes on a Mac:",
		steps: [
			{ text: "Install Apple's command line tools.", command: 'xcode-select --install' },
			{
				text: 'Install Rust and follow its prompts.',
				command: "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
			},
			{
				text: 'Open a new Terminal window, then download and build it. The first build takes a few minutes.',
				command:
					'git clone https://github.com/cutenode/localnetwork.git\ncargo install --path localnetwork/apps/locallibrary --locked'
			},
			{ text: 'Plug in your drives and set it up.', command: 'locallibrary setup' }
		]
	},
	sections: [
		{
			title: 'Signing in',
			markdown: `locallibrary signs in with your Atmosphere account, the one you use for Bluesky and other AT Protocol apps. You sign in on your account's own server, in your browser, so locallibrary never sees your password.

Signing in only asks who you are. Sign out with \`locallibrary account logout\`, or remove locallibrary in your account's settings, at any time. Servers read its details from [its client document](https://locallibrary.localnetwork.computer/oauth-client-metadata.json).`
		},
		{
			title: 'Publishing',
			markdown: `The first time you publish, locallibrary asks for one more permission: to create and update three kinds of record in your account. It can't delete them, and can't touch anything else.

- \`locallibrary sets publish\` writes a set: the day, venue, event and tracks in order.
- \`locallibrary settings publish\` writes your My Settings.
- \`locallibrary profile publish\` writes a rounded summary of how you play.

Each one shows what it will publish and asks first. Tracks you mark unnamed go out without a name, and a set's notes, recordings and file paths stay on your computer. What's published is public.

A mix exported with \`locallibrary mix … --export\` can go on your [Local Network profile](/for-djs), with its tracklist.`
		}
	],
	docs: loadDocs(
		'apps/locallibrary',
		import.meta.glob<string>('../../../../locallibrary/{README.md,docs/*.md}', {
			query: '?raw',
			import: 'default',
			eager: true
		})
	),
	hosts: () => [(process.env.LOCALLIBRARY_HOST || 'locallibrary.localnetwork.computer').toLowerCase()],
	clientMetadata
};
