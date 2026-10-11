import { loadDocs } from './docs.ts';
// Written by locallibrary, which signs in as https://<host>/oauth-client-metadata.json:
//   locallibrary account client-metadata -o apps/web/src/lib/tools/locallibrary/oauth-client-metadata.json
import clientMetadata from './locallibrary/oauth-client-metadata.json?raw';
import type { Tool } from './types.ts';

export const locallibrary: Tool = {
	id: 'locallibrary',
	name: 'locallibrary',
	kind: 'Command line tool',
	tagline: 'Keeps your rekordbox USB drives in sync, backed up and edited.',
	summary:
		'A command line tool and terminal player for DJs who export from rekordbox to a stick and play it on CDJs or XDJs. It runs on a Mac (Linux and Windows are experimental). Every change shows its plan and asks first, and undo takes the last one back.',
	features: [
		'Keeps every copy of your library identical to the drive you export to',
		'Tells you which drive is newest, and why',
		'Keeps a history of your library, so a lost drive can be rebuilt',
		'Edits playlists, tags, cues, ratings and cover art right on your drives',
		"Checks drives before a set, down to the tracks a player won't load",
		'Turns the history players record into tracklists for Mixcloud, YouTube or a CUE sheet',
		'Sets the same My Settings on every drive'
	],
	install: {
		note: "There's no download yet, so for now you build it on your own computer. On a Mac:",
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
