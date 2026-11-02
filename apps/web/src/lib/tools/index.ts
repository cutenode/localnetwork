// The Local Network tools with a page on this site. A new tool is one file here, listed below.
// Server-only: the docs are bundled in, so only server code (loads, hooks) and Storybook import this.
import { trackSchema } from '#lib/mix/schema.ts';
import exported from './captures/locallibrary/demo-track.json' with { type: 'json' };
import { locallibrary } from './locallibrary.ts';
import { localtoolbox } from './localtoolbox.ts';
import type { Tool } from './types.ts';

export const tools: Tool[] = [locallibrary, localtoolbox];

export const findTool = (id: string): Tool | undefined => tools.find((tool) => tool.id === id);

/**
 * The track each tool page's hot cues sit on: rekordbox's own analysis of its demo track, as
 * locallibrary exports it (`pnpm atstore` writes it with the captures). Only what the waveform draws.
 */
export const demoTrack = (({ durationSecs, overview, phrases }) => ({ durationSecs, overview, phrases }))(
	trackSchema.parse(exported)
);

/** What /tools and the home page show of each tool: its first three things, and the capture of the first. */
export const toolCards = () =>
	tools.map(({ id, name, kind, tagline, cues }) => ({
		id,
		name,
		kind,
		tagline,
		lines: cues.slice(0, 3).map((cue) => cue.line),
		capture: cues[0].capture
	}));
