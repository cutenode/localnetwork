import { vi } from 'vitest';

/**
 * Stands in for the server behind a `use:enhance` form: keeps what each submission sent, and holds the answer until
 * `answer()`. The answer is an error result, which `enhance` applies without a running SvelteKit app (there's no route
 * whose error page it could show), so the form's own code after the save runs as it would on a page. Undo it with
 * `vi.unstubAllGlobals()`.
 */
export function stubFormAction() {
	const sent: FormData[] = [];
	const urls: string[] = [];
	const waiting: (() => void)[] = [];

	vi.stubGlobal('fetch', async (input: URL, init: RequestInit) => {
		sent.push(await new Request(input, init).formData());
		urls.push(String(input));

		return new Promise<Response>((resolve) => {
			waiting.push(() => resolve(Response.json({ type: 'error', error: { message: 'Down' } }, { status: 500 })));
		});
	});

	return {
		sent,
		urls,
		answer: () => {
			for (const resolve of waiting.splice(0)) resolve();
		}
	};
}
