import { type LocallibraryStatus, setName } from '#lib/dj.ts';
import { setHref } from '#lib/format.ts';
import type { AccountView } from '#lib/types.ts';

/**
 * What the dashboard home's locallibrary card says about `account`: when their stats and sets were last published, and
 * their newest set by the day it was played. Read from the records locallibrary publishes, as DjService keeps them.
 */
export async function locallibraryStatus(
	{ app }: { app: { dj: Pick<App.Locals['app']['dj'], 'fresh' | 'activity'> } },
	account: AccountView
): Promise<LocallibraryStatus> {
	const [activity] = app.dj.fresh([account]) ?? (await app.dj.activity([account]));
	const [newest] = activity?.sets ?? [];

	const at = (activity?.sets ?? []).reduce<string | null>(
		(latest, { createdAt }) =>
			createdAt && (!latest || Date.parse(createdAt) > Date.parse(latest)) ? createdAt : latest,
		null
	);

	return {
		statsAt: activity?.profile?.generatedAt ?? null,
		sets: newest ? { href: setHref(newest.uri, account.handle), name: setName(newest), at } : null
	};
}
