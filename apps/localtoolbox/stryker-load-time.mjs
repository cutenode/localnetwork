// A Stryker ignore-plugin (see stryker.config.mjs) for the code a module runs
// as it loads: its top-level tables, patterns and keys, outside any function.
//
// Most test files import those modules before Stryker can switch a mutant on,
// so a mutant there is never really in play: measured on title-case.ts, it
// survived whatever the tests said or ran out the clock and was counted as
// killed, and either way it cost a rerun of most of the suite. Everything
// inside a function, which runs later, is still mutated.

/** Whether a function sits anywhere inside the node, whose body must not be skipped with it. */
function holdsFunction(path) {
	let found = false;
	path.traverse({
		Function(inner) {
			found = true;
			inner.stop();
		}
	});

	return found;
}

export const strykerPlugins = [
	{
		kind: 'Ignore',
		name: 'load-time',
		value: {
			shouldIgnore(path) {
				const runsLater = path.isFunction() || path.findParent((parent) => parent.isFunction()) || holdsFunction(path);

				return runsLater ? undefined : 'Runs as the module loads, before a mutant can be switched on';
			}
		}
	}
];
