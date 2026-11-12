import { expect, it } from 'vitest';
import { load } from './+page.ts';
import type { PageData, PageLoadEvent } from './$types';

const threads: PageData['threads'] = [];

const opened = (search: string) =>
	// SAFETY: the load reads only these two.
	load({ data: { threads }, url: new URL(`https://localnetwork.test/dashboard/bookings${search}`) } as PageLoadEvent);

it('reads the act filter from the address, and passes the server data on', async () => {
	expect(await opened('?project=neon-tide')).toEqual({ threads, filter: 'neon-tide' });
	expect(await opened('')).toEqual({ threads, filter: null });
});
