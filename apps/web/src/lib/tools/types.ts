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

/** A tool in the Local Network family, with its page at `/tools/<id>`. */
export type Tool = {
	id: string;
	name: string;
	/** What sort of thing it is, such as a Chrome extension. */
	kind: string;
	tagline: string;
	summary: string;
	/** What it does, one line each. */
	features: string[];
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
