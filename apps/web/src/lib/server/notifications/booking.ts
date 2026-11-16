import { z } from 'zod';
import type { Line } from './kind';

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

/** Who acted, as the middle of a sentence says it. */
export const who = ({ from, projectName }: z.output<typeof booking>) =>
	from === 'act' ? (projectName ?? 'the DJ') : 'the promoter';

export const show = ({ title }: z.output<typeof booking>) => title ?? 'Booking inquiry';

export const bookingLine = (data: z.output<typeof booking>, did: Line['did']): Line => ({
	subject: show(data),
	scope: data.projectName ?? show(data),
	who: who(data),
	did
});
