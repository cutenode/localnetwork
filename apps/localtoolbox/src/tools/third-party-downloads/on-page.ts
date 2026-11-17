// Gates done on their own page: the background worker's half of a page site
// (`how: 'page'` in sites/index.ts).
//
// The Downloader's Get, Get all, and the Download button on a SoundCloud page
// hand the worker a run of gates. It opens the first in a tab and keeps the
// rest; gate-page.ts, the content script on those sites, asks it whether its
// tab is the one and what to help with there. The person does the steps. When
// the page starts its download, the worker files it into the site's folder,
// marks the track and opens the next gate in the same tab. On a sign-up page
// (Laylo) the person's press of the page's own button is the end of it, and
// the bar on the page offers the next one.
//
// A download counts as the gate's when it starts while that gate is open and
// its referrer or its address is on the gate's site: Droploud's page clicks a
// link to the file, and Pump Your Sound's form answers with it, so either
// carries the page as its referrer. One the worker started itself (a walked
// Hypeddit file) is never claimed, and nothing else is renamed.
//
// The run lives in chrome.storage.session, so a worker that sleeps between
// two clicks wakes up knowing it. What became of each gate goes beside it,
// under its URL, for the Downloader's rows to read (walked.svelte.ts): the
// panel is closed while all this happens, and the worker never writes the
// panel's own list.

import type { GateOutcome, GatePageHelp, GatePagePrefs, GatePageSaved, PageGate } from '$lib/types';
import { fallback, z } from '$lib/zod';
import { safeName } from './save.ts';
import { hostOf, openUrlOf, siteFor, type PageSite } from './sites/index.ts';

const RUN = 'gate-run';

/** Where outcomes are kept, by gate URL. Read by walked.svelte.ts. */
export const OUTCOMES = 'gate-outcomes';

/** How long a saved file's bar shows before the next gate opens. */
export const NEXT_AFTER_MS = 2500;

// Both are written only here, so a record that does not read is no record.
const Run = z.object({
	tab: z.number(),
	/** The open gate first, then the rest. */
	gates: z.array(z.object({ url: z.string(), permalink: z.string() })).check(z.minLength(1)),
	/** How many gates the run was handed. */
	of: z.number()
}) satisfies z.ZodMiniType<{ tab: number; gates: PageGate[]; of: number }>;

type Run = z.infer<typeof Run>;

/** What is kept under OUTCOMES, read back: every outcome by gate URL, or none. */
export const Outcomes = fallback(
	z.record(z.string(), z.object({ state: z.enum(['opened', 'done', 'sent']), detail: z.string(), label: z.string() })),
	{}
) satisfies z.ZodMiniType<Record<string, GateOutcome>>;

/** Reads the settings a page gate uses, which only the offscreen page can. */
export type Prefs = () => Promise<GatePagePrefs>;

const pageSiteOf = (url: string) => {
	const site = siteFor(url);

	return site?.how === 'page' ? site : null;
};

const onSite = (site: PageSite, url: string | undefined) => {
	const host = hostOf(url ?? '');

	return site.hosts.some((own) => host === own || host.endsWith(`.${own}`));
};

async function readRun(): Promise<Run | null> {
	try {
		const run = Run.safeParse((await chrome.storage.session.get(RUN))[RUN]);

		return run.success ? run.data : null;
	} catch (_e) {
		return null;
	}
}

async function writeRun(run: Run | null) {
	await (run ? chrome.storage.session.set({ [RUN]: run }) : chrome.storage.session.remove(RUN));
}

async function setOutcome(urls: string[], outcome: GateOutcome | null) {
	const kept = Outcomes.parse((await chrome.storage.session.get(OUTCOMES))[OUTCOMES]);

	for (const url of urls) {
		if (outcome) kept[url] = outcome;
		else delete kept[url];
	}

	await chrome.storage.session.set({ [OUTCOMES]: kept });
}

/** The gate's own site, for a gate a run holds: only page gates get into one. */
const siteOf = (gate: PageGate) => pageSiteOf(gate.url)!;

const opened = (gate: PageGate) =>
	setOutcome([gate.url], { state: 'opened', detail: `finish it on ${siteOf(gate).name}'s page.`, label: '' });

/** The worker's side of page gates, given the offscreen page's settings and marks. */
export function onPage(prefs: Prefs) {
	/**
	 * Opens the first gate in a tab, beside `from` when there is one, and keeps the rest for after
	 * it. A run already going is replaced: the newest Get is what the person asked for.
	 */
	async function open(gates: PageGate[], from?: chrome.tabs.Tab) {
		const doable = gates.filter((gate) => pageSiteOf(gate.url));

		if (doable.length === 0) return;
		await setOutcome(
			doable.map((gate) => gate.url),
			null
		);

		const where: chrome.tabs.CreateProperties = { url: openUrlOf(doable[0].url) };

		if (from) {
			where.index = from.index + 1;
			where.openerTabId = from.id;
		}

		const tab = await chrome.tabs.create(where);

		await writeRun({ tab: tab.id!, gates: doable, of: doable.length });
		await opened(doable[0]);
	}

	/** The next gate in the same tab, or the end of the run. `tab` is the one asking, if a page asked. */
	async function next(tab?: chrome.tabs.Tab) {
		const run = await readRun();

		if (!run || (tab && tab.id !== run.tab)) return;
		const rest = run.gates.slice(1);

		if (rest.length === 0) {
			await writeRun(null);

			return;
		}

		await writeRun({ ...run, gates: rest });

		try {
			await chrome.tabs.update(run.tab, { url: openUrlOf(rest[0].url), active: true });
			await opened(rest[0]);
		} catch (_e) {
			// The tab went between the click and the move.
			await writeRun(null);
		}
	}

	/** What gate-page.ts should help with in `tab`, or null when it is not the run's. */
	async function hello(tab: chrome.tabs.Tab | undefined, url: string | undefined): Promise<GatePageHelp | null> {
		const run = await readRun();

		if (!run || tab?.id !== run.tab) return null;
		const site = siteOf(run.gates[0]);

		if (!onSite(site, url)) return null;

		return {
			name: site.name,
			say: site.page.say,
			steps: site.page.steps,
			email: site.page.email ? (await prefs()).email : '',
			emailField: site.page.email ?? '',
			signup: site.page.signup ?? '',
			at: run.of - run.gates.length + 1,
			of: run.of
		};
	}

	/** The person submitted the page's sign-up form. */
	async function signedUp(tab: chrome.tabs.Tab | undefined) {
		const run = await readRun();

		if (!run || tab?.id !== run.tab) return;
		await setOutcome([run.gates[0].url], {
			state: 'sent',
			detail: `you RSVP'd on ${siteOf(run.gates[0]).name}'s page. the link comes by mail.`,
			label: ''
		});
	}

	/** Help stops on this tab, and the gates after it are left as they were. */
	async function stop(tab: chrome.tabs.Tab | undefined) {
		const run = await readRun();

		if (run && tab?.id === run.tab) await writeRun(null);
	}

	/** A tab closed: if it was the run's, the run is over. */
	async function closed(tabId: number) {
		const run = await readRun();

		if (run?.tab === tabId) await writeRun(null);
	}

	/**
	 * Where a download the page started goes, if it is the open gate's file: its site's folder.
	 * Undefined for any other download, which keeps the name the browser gave it.
	 */
	async function file(item: chrome.downloads.DownloadItem): Promise<chrome.downloads.FilenameSuggestion | undefined> {
		if (item.byExtensionId === chrome.runtime.id) return undefined;
		const run = await readRun();

		if (!run) return undefined;
		const gate = run.gates[0];
		const site = siteOf(gate);

		if (![item.referrer, item.url, item.finalUrl].some((url) => onSite(site, url))) return undefined;

		// What the browser would call it, which is the site's own name for it.
		const name = safeName(item.filename.replace(/^.*[\\/]/, '')) || 'download';
		const path = `${site.subfolder}/${name}`;

		// The download waits on the answer, so the rest happens after it.
		void filed(run, gate, name, path);

		return { filename: path, conflictAction: 'uniquify' };
	}

	async function filed(run: Run, gate: PageGate, name: string, path: string) {
		const more = run.gates.length > 1;

		await setOutcome([gate.url], { state: 'done', detail: '', label: name });
		const { mark } = await prefs();

		// The mark is the page's to make: the worker has no copy of the marks
		// store (downloaded-store.ts), and gate-page.ts loads it beside itself,
		// as content.ts does on SoundCloud.
		try {
			await chrome.tabs.sendMessage(run.tab, {
				type: 'GATE_PAGE_SAVED',
				path,
				more,
				mark: mark ? gate.permalink : ''
			} satisfies GatePageSaved);
		} catch (_e) {
			// The page has gone, and the file still lands, unmarked.
		}

		if (more) setTimeout(() => void next(), NEXT_AFTER_MS);
		else await writeRun(null);
	}

	return { open, next, hello, signedUp, stop, closed, file };
}
