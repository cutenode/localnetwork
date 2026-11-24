import { error } from '@sveltejs/kit';

type Notice = { title: string; message: string };

/**
 * Hides a page about the accounts in `dids` once none of them is active (ADR 0020): 410 when every one was deleted,
 * else 404, each saying so in `copy` without saying why. One that's only deactivated can come back, so a mix of
 * deleted and inactive accounts answers 404. A page with no accounts to ask stays up.
 */
export async function hideUnlessActive(
	app: App.Locals['app'],
	dids: readonly string[],
	copy: { deleted: Notice; inactive: Notice }
): Promise<void> {
	const statuses = await Promise.all(dids.map((did) => app.core.accountStatuses.status(did)));

	if (statuses.length === 0 || statuses.includes('active')) return;

	if (statuses.every((status) => status === 'deleted')) error(410, { status: 410, ...copy.deleted });

	error(404, { status: 404, ...copy.inactive });
}
