// The settings sheet's "report a bug" link: GitHub's issue form for this
// extension (.github/ISSUE_TEMPLATE/localtoolbox-bug.yml at the repo root),
// opened with what the popup already knows filled in. A form field is filled
// by a query parameter named after its `id`, so nobody has to go and find the
// version on chrome://extensions or work out which browser they are in.
//
// The website's own report form describes a browser the same way
// (`browserLabel` in apps/web/src/lib/server/services/bug-reports.ts), so
// reports from both read alike. The website has only a User-Agent header to
// go on and parses it; the extension runs in Chromium alone, which says the
// same thing outright in navigator.userAgentData.

import { TOOLS } from './tools.ts';

const NEW_ISSUE = 'https://github.com/cutenode/localnetwork/issues/new';

/** The manifest's version, and the commit it was built from when the build knew it: "2.11.0 (be03543)". */
export function extensionVersion(): string {
	const { version } = chrome.runtime.getManifest();
	const commit = import.meta.env.VITE_COMMIT;

	return commit ? `${version} (${commit})` : version;
}

// Besides the browser itself, the brand list names "Chromium" for the engine
// and a made-up brand such as "Not A;Brand", there to break code that expects
// a fixed list. The browser is the brand that is neither.
const ENGINE = 'Chromium';

const MADE_UP = /not.a.brand/i;

/** "Chrome 141, macOS" from what the browser says about itself, or undefined when it says nothing. */
export function browserLabel(agent: NavigatorUAData | undefined): string | undefined {
	if (!agent) return undefined;
	const brands = agent.brands.filter(({ brand }) => !MADE_UP.test(brand));
	const browser = brands.find(({ brand }) => brand !== ENGINE) ?? brands[0];

	if (!browser) return undefined;

	// Chrome calls itself "Google Chrome" here and "Chrome" in a User-Agent, which is what the
	// website's reports say. The version is the major one only, and the OS is its name alone: the
	// low-entropy hints, unlike a User-Agent, carry nothing more.
	const name = browser.brand.replace(/^Google /, '');

	return [`${name} ${browser.version}`, agent.platform].filter(Boolean).join(', ');
}

/**
 * The form's address, with the version, the browser and, when a tool is open
 * under the sheet, that tool picked in "Which part?". The dropdown's options
 * are the tools' names as the registry has them.
 */
export function bugReportUrl(openTool: string | null): string {
	const url = new URL(NEW_ISSUE);

	const fields = {
		template: 'localtoolbox-bug.yml',
		tool: TOOLS.find((tool) => tool.id === openTool)?.name,
		version: extensionVersion(),
		browser: browserLabel(navigator.userAgentData)
	};

	for (const [id, value] of Object.entries(fields)) if (value) url.searchParams.set(id, value);

	return url.href;
}
