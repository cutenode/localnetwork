// The set of tracks the user has downloaded, shared by the content script
// (which paints the "Downloaded" markers and notices SoundCloud's own download
// buttons) and the Manager panel (which marks tracks when a link is opened).
//
// Lives in the extension's own storage (chrome.storage.local), which the
// page's scripts cannot read, as a map of permalink path ("/user/track") to
// the ISO time the track was marked. Nothing here touches the page or talks
// to SoundCloud.
//
// Loaded before content.ts on soundcloud.com and by the panel page; both use
// the TbxDownloaded global it defines.

const TbxDownloaded = (() => {
	type DownloadedMarks = Tbx.DownloadedMarks;

	const KEY = 'downloaded' as const;

	// Whether what storage holds under KEY is the marks. Nothing but this file
	// writes there, and it writes permalink -> time; but an array is an object
	// too, and would take marks as named properties that storage then drops on
	// the way in, so it counts as no map at all, as does anything that is not
	// an object.
	const isMarks = (stored: unknown): stored is DownloadedMarks =>
		typeof stored === 'object' && stored !== null && !Array.isArray(stored);

	const load = async (): Promise<DownloadedMarks> => {
		const stored = await chrome.storage.local.get(KEY);

		return isMarks(stored[KEY]) ? stored[KEY] : {};
	};

	// Marks (value true) or unmarks (false) several tracks in one write.
	/** @param permalinks empty ones are skipped. */
	const setMany = async (permalinks: string[], value: boolean) => {
		const list = permalinks.filter(Boolean);

		if (list.length === 0) return;
		const map = await load();
		const now = new Date().toISOString();

		for (const permalink of list) {
			if (value) map[permalink] = now;
			else delete map[permalink];
		}

		await chrome.storage.local.set({ [KEY]: map });
	};

	const set = (permalink: string, value: boolean) => setMany([permalink], value);

	// Merges marks that came from somewhere else (an exported state file, see
	// state-transfer.js) into the stored ones, keeping the times they carry
	// rather than stamping them now. A track already marked keeps the earlier
	// of the two times, so re-importing an old backup never makes a mark look
	// newer than it is. Earlier as instants rather than as text: a file may
	// write a time with an offset ("…T13:00:00+02:00"), which sorts after
	// "…T12:00:00.000Z" though it is an hour before it. Returns how many marks
	// were added.
	const merge = async (marks: DownloadedMarks) => {
		const entries = Object.entries(marks);

		if (entries.length === 0) return 0;
		const map = await load();
		let added = 0;

		for (const [permalink, at] of entries) {
			const current = map[permalink];

			if (current === undefined) added++;

			if (current === undefined || Date.parse(at) < Date.parse(current)) map[permalink] = at;
		}

		await chrome.storage.local.set({ [KEY]: map });

		return added;
	};

	// Puts these marks in place of every mark there is, for an import asked to
	// *replace* what is here rather than add to it (state-transfer.js). Nothing
	// merges and nothing survives: a track marked here but not in the file comes
	// out unmarked, which is the whole difference between the two imports.
	// Returns how many marks there are afterwards.
	const replaceAll = async (marks: DownloadedMarks) => {
		const map = { ...marks };
		await chrome.storage.local.set({ [KEY]: map });

		return Object.keys(map).length;
	};

	// Removes every mark.
	const clear = () => chrome.storage.local.set({ [KEY]: {} });

	return { KEY, isMarks, load, set, setMany, merge, replaceAll, clear };
})();
