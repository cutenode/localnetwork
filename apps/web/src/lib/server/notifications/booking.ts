import { z } from 'zod';

/**
 * Something in a booking thread, the data every booking kind keeps. It names the act, never the manager who wrote,
 * since a manager may be a hidden member (docs/adr/0004), and it holds none of what was written, which stays in the
 * thread's private space.
 */
export const booking = z.object({
	/** The thread's key, which its page is at. */
	skey: z.string(),
	/** The show's title, from the inquiry. */
	title: z.string().nullable(),
	projectName: z.string().nullable(),
	/** Which side of the thread did it. */
	from: z.enum(['promoter', 'act'])
});

export const who = ({ from, projectName }: z.output<typeof booking>) =>
	from === 'act' ? (projectName ?? 'The DJ') : 'The promoter';

export const show = ({ title }: z.output<typeof booking>) => title ?? 'Booking inquiry';
