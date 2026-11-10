/** Something waiting on the viewer, as Home lists it. `scope` is the act or thing it belongs to, for its monogram. */
export type NeedsRow = { key: string; scope: string; title: string; detail: string; href: string; action: string };

/** A night the viewer has something to do with. `tentative` nights aren't settled yet; `when` says how close it is. */
export type NightRow = {
	key: string;
	date: string;
	title: string;
	detail: string;
	href: string;
	tentative: boolean;
	when?: 'Tonight' | 'Tomorrow';
};

/** Home's heading: the one count of what's waiting on the viewer. */
export const statusLine = (count: number): string => {
	if (!count) return 'Nothing needs you.';

	return count === 1 ? '1 thing needs you.' : `${count} things need you.`;
};
