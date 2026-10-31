// The Local Network tools with a page on this site. A new tool is one file here, listed below.
// Server-only: the docs are bundled in, so only server code (loads, hooks) and Storybook import this.
import { locallibrary } from './locallibrary.ts';
import { localtoolbox } from './localtoolbox.ts';
import type { Tool } from './types.ts';

export const tools: Tool[] = [locallibrary, localtoolbox];

export const findTool = (id: string): Tool | undefined => tools.find((tool) => tool.id === id);

/** What the home page's tool cards show: no docs, and each tool's first three features. */
export const toolCards = () =>
	tools.map(({ id, name, kind, tagline, features }) => ({ id, name, kind, tagline, features: features.slice(0, 3) }));
