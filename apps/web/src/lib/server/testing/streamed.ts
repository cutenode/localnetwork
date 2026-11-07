export const MEBIBYTE = 1024 * 1024;

/** A body of `mebibytes`, made as it's read, so a test can see how much of it a reader took. */
export function streamed(mebibytes: number) {
	const chunk = new Uint8Array(MEBIBYTE);
	let made = 0;

	return {
		body: new ReadableStream<Uint8Array>({
			pull(controller) {
				if (made === mebibytes * MEBIBYTE) return controller.close();
				made += MEBIBYTE;
				controller.enqueue(chunk);
			}
		}),
		made: () => made
	};
}
