// The settings sheet's "report a bug" link: GitHub's issue form for this
// extension (.github/ISSUE_TEMPLATE/localtoolbox-bug.yml at the repo root),
// opened with what the popup already knows filled in. A form field is filled
// by a query parameter named after its `id`, so nobody has to go and find the
// version on chrome://extensions or work out which browser they are in.
//
// The website's own report form describes a browser the same way
// (`browserLabel` in apps/web/src/lib/server/services/bug-reports.ts), so
// reports from both read alike.

import Bowser from 'bowser';
import { TOOLS } from './tools.ts';

const NEW_ISSUE = 'https://github.com/cutenode/localnetwork/issues/new';

/** The manifest's version, and the commit it was built from when the build knew it: "2.11.0 (be03543)". */
export function extensionVersion(): string {
	const { version } = chrome.runtime.getManifest();
	const commit = import.meta.env.VITE_COMMIT;

	return commit ? `${version} (${commit})` : version;
}

/** "Chrome 141, macOS" from a User-Agent, or undefined when it names no browser. */
export function browserLabel(userAgent: string): string | undefined {
	if (!userAgent) return undefined;
	const { browser, os } = Bowser.parse(userAgent);

	if (!browser.name) return undefined;
	const major = browser.version?.split('.')[0];

	// Browsers freeze the OS version in the User-Agent (Chrome says macOS 10.15 on every Mac), so only its name goes in.
	return [[browser.name, major].filter(Boolean).join(' '), os.name].filter(Boolean).join(', ');
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
		browser: browserLabel(navigator.userAgent)
	};

	for (const [id, value] of Object.entries(fields)) if (value) url.searchParams.set(id, value);

	return url.href;
}
