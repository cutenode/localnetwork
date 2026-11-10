/**
 * A load's value that may still be on its way: what it resolved to once it has, and `initial` until then, so a streamed
 * value can join what's on the page instead of replacing a block of it. A value that's already here counts from the
 * first render, the server's included. A newer promise takes over from one still waiting.
 */
export function resolved<T, I = T>(load: () => T | Promise<T>, initial: I) {
	const first = load();
	let value = $state<T | I>(first instanceof Promise ? initial : first);

	$effect(() => {
		let live = true;
		void Promise.resolve(load()).then((next) => {
			if (live) value = next;
		});

		return () => {
			live = false;
		};
	});

	return {
		get value() {
			return value;
		}
	};
}
