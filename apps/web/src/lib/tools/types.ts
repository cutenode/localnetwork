import type { Picture } from '@sveltejs/enhanced-img';

/** One of a tool's guides: its README or a file in its `docs/`, as Markdown loaded at build time. */
export type ToolDoc = {
	/** The guide's path on the site, `/tools/<tool>/docs/<slug>`: the file name without `.md`, `readme` for the README. */
	slug: string;
	/** Where the file lives in the repo, which relative links in it resolve against. */
	path: string;
	title: string;
	/** The Markdown without its `# title` line, which the page shows itself. */
	markdown: string;
};

export type InstallStep = { text: string; command?: string };

/**
 * A picture of the tool at work, from `captures/<tool>/` (which `pnpm atstore` takes), imported with
 * `?enhanced` so it ships as AVIF and WebP with its size set. `phone` is the same view taken at a
 * phone's width, shown there instead.
 */
export type Capture = { picture: Picture; phone?: Picture; alt: string };

/** Something the tool does: a hot cue on its page, with one line and the capture that shows it. */
export type Cue = { name: string; line: string; capture: Capture };

/** A tool in the Local Network family, with its page at `/tools/<id>`. */
export type Tool = {
	id: string;
	name: string;
	/** What sort of thing it is, such as a Chrome extension. */
	kind: string;
	tagline: string;
	summary: string;
	/** What it does, in the order its page's hot cues show it. The first is the hero on /tools. */
	cues: Cue[];
	install: { note?: string; steps: InstallStep[] };
	/** More sections for the tool's page, in Markdown. */
	sections?: { title: string; markdown: string }[];
	/** The README first, then the guides in the order the README links them. */
	docs: ToolDoc[];
	/** The hosts this app answers for the tool (read per request, so an environment variable can override them). */
	hosts?: () => string[];
	/** The OAuth client document served at `/oauth-client-metadata.json` on the tool's hosts. */
	clientMetadata?: string;
};
