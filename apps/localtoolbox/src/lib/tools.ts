// The tools the popup offers, in the order the menu lists them.
//
// The session queue is not among them and never was a tool in spirit: it is
// about the other tools rather than beside them, so it lives in the header of
// every view instead (QueueButton.svelte, QueueSheet.svelte, queue-sheet.ts).
// What it still reads from here is the registry — the two tools it can hand a
// batch to name themselves.
//
// To add a tool: write src/tools/<id>/Panel.svelte and register it here. The
// entry is all the shell needs — the menu, the header, the colour of every
// tinted thing in the panel and the route it lives at are read from it.
//
// `brand` is the tool's own colour, written onto <html> while its panel is
// open; everything else (the primary buttons, the focus ring, the header
// wash, the icon tile) is derived from it in app.css.
//
// `load` rather than the panel itself: the panels and the component library
// under them are the bulk of the bundle, and a popup that opens on the menu —
// or on one tool — has no use for the others. Each is its own
// chunk, fetched when its tool is opened (and speculatively, at boot, for the
// tool the popup was left on: see loadPanel and $lib/view.svelte.ts).
// Everything the menu draws — the name, the icon, the colour — is here, so
// nothing has to be loaded to draw it.
//
// `warm` is the slow thing the tool's panel will do first, started before the
// panel exists. Reading a page is round trips — the browser process, then the
// content script, then in the Manager tool a good many requests to
// SoundCloud — and a chunk compiling is not a reason for them to wait. The
// shell calls it as soon as it knows which tool is opening; the panel then
// asks for the page as it always did and is handed the read already running
// (see $lib/soundcloud.ts). A tool with nothing slow to start has no `warm`.
//
// The Manager tool warms the cheap read only. Its list is its own and nothing
// goes onto it by itself, so what the tab holds is read to *describe* the add
// button — a few milliseconds of reading the page's DOM, and no request to
// SoundCloud — and asking SoundCloud where those tracks can be had waits for
// somebody to press it.

import type { Component } from 'svelte';
import type { PanelProps, Tool } from '$lib/types';
import { warmActiveTab } from './chrome.ts';
import { warmPage } from './soundcloud.ts';

import ListMusic from '@lucide/svelte/icons/list-music';
import Download from '@lucide/svelte/icons/download';
import LockOpen from '@lucide/svelte/icons/lock-open';
import Users from '@lucide/svelte/icons/users';

export type PanelComponent = Component<PanelProps>;

export const MENU_BRAND = 'oklch(0.56 0.2 285)';

export const TOOLS: Tool[] = [
	{
		id: 'soundcloud-playlist-to-text',
		name: 'SoundCloud playlist to text',
		short: 'playlist to text',
		description: 'a playlist, likes or track page as text you can paste.',
		icon: ListMusic,
		brand: 'oklch(0.68 0.19 45)',
		load: () => import('../tools/soundcloud-playlist-to-text/Panel.svelte'),
		warm: () => warmPage('EXTRACT_TRACKS')
	},
	{
		id: 'manager',
		name: 'Manager',
		short: 'manager',
		description: 'list tracks from playlist, likes and track pages, and find their download links.',
		icon: Download,
		brand: 'oklch(0.72 0.17 68)',
		load: () => import('../tools/manager/Panel.svelte'),
		warm: () => warmPage('EXTRACT_TRACKS')
	},
	{
		id: 'third-party-downloads',
		name: 'third-party downloader',
		short: 'downloader',
		description: 'walk the gate links behind a page and save the files.',
		icon: LockOpen,
		brand: 'oklch(0.62 0.15 238)',
		load: () => import('../tools/third-party-downloads/Panel.svelte'),
		warm: () => warmActiveTab()
	},
	{
		id: 'artists',
		name: 'artists',
		short: 'artists',
		description: 'every artist behind your tracks, with their socials.',
		icon: Users,
		brand: 'oklch(0.62 0.19 345)',
		load: () => import('../tools/artists/Panel.svelte'),
		// The same cheap read the Manager warms, and for the same reason: the
		// tab in front is read to describe the add button, and asking SoundCloud
		// about anybody waits for somebody to press it.
		warm: () => warmPage('EXTRACT_TRACKS')
	}
];

/** @returns null for an id this version has no tool for */
export const toolById = (id: string): Tool | null => TOOLS.find((tool) => tool.id === id) ?? null;

// One chunk per tool, fetched at most once however often its tool is opened
// and closed: going back to the menu and into the same tool again is then as
// instant as it was when every panel was in the one bundle.
const chunks = new Map<string, Promise<PanelComponent>>();

/**
 * The panel component for a tool, loading its chunk if this is the first ask.
 *
 * @returns null for an id this version has no tool for — a session left by a later one, say.
 */
export function loadPanel(id: string): Promise<PanelComponent> | null {
	if (chunks.has(id)) return chunks.get(id)!;
	const tool = toolById(id);

	if (!tool) return null;

	// A chunk that failed to arrive is not remembered as a failure: the network
	// is not involved (the file is in the extension), but a retry costs nothing
	// and a permanently broken tool would cost the popup.
	const chunk = tool.load().then(
		(module) => module.default,
		(error) => {
			chunks.delete(id);
			throw error;
		}
	);

	chunks.set(id, chunk);

	return chunk;
}

/** Paints the whole popup in a tool's colour (or the menu's when given none). */
export function applyBrand(brand?: string) {
	document.documentElement.style.setProperty('--brand', brand || MENU_BRAND);
}
