import { setFlagsFromString } from 'node:v8';
import { runInNewContext } from 'node:vm';

// V8 gives a new context `gc` once the flag is set, so tests can collect without starting node with --expose-gc.
setFlagsFromString('--expose-gc');

// SAFETY: with --expose-gc set, a new context's global `gc` is V8's collector, which takes no arguments.
const collect = runInNewContext('gc') as () => void;

/**
 * What `pending` settles to, with garbage collected every few milliseconds until it does, as in a busy process where
 * collection runs while a request waits. Anything a request needs that only a weak reference holds goes missing.
 */
export async function collectingGarbage<T>(pending: Promise<T>): Promise<T> {
	const timer = setInterval(collect, 5);

	try {
		return await pending;
	} finally {
		clearInterval(timer);
	}
}
