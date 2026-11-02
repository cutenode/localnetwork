// Putting a long list into the DOM a frame's worth at a time.
//
// A Manager row is a picture, a title, three or four buttons and their
// tooltips: in Chrome each one is a couple of milliseconds to mount, so five
// hundred are the best part of a second of DOM work, and the popup shows about
// eight at a time. The first paint draws a screenful and the rest arrive over
// the following frames: the same work, done behind the list rather than in
// front of it.
//
// The search box needs the same thing. Typing only takes rows away, which is
// cheap, but deleting a search puts every row it hid back at once: with three
// hundred tracks a single Backspace froze the input for nearly half a second.
// So the budget is per frame rather than per list: rows already on screen stay
// where they are, and the ones joining them, from a longer list or a shorter
// search, come in a frame's worth at a time. A frame's worth is small, so
// the keystroke that brings rows back waits on forty of them at most.

/** Rows the first paint puts up: a screenful, with room to scroll. */
const FIRST_PAINT = 60;

/** Rows each paint after it may add to what is on screen. */
const PER_FRAME = 40;

/**
 * Which of a list's rows are on screen so far.
 *
 * @param keys every row's key, in the order the list draws them
 */
export function createPainting(keys: () => string[]) {
	// The rows on screen when this frame began. Not state: it describes paints
	// that have happened, and nothing should redraw because it moved. A list
	// that changes twice within a frame is painted from the same start both
	// times, so a frame adds its budget once however often the list moves.
	let settled: ReadonlySet<string> = new Set();

	// Each frame after the first lets another PER_FRAME rows in.
	let frames = $state(0);

	const painted = $derived.by(() => {
		let left = frames === 0 ? FIRST_PAINT : PER_FRAME;

		return new Set(keys().filter((key) => settled.has(key) || left-- > 0));
	});

	// A frame if there is one, a timeout if there is not. `requestAnimationFrame`
	// is what says "after the paint", which is the whole point of chunking, but
	// it does not run at all while the page is hidden, and a list that stopped
	// growing there would still be sixty rows long when it came back.
	$effect(() => {
		if (painted.size >= keys().length) {
			settled = painted;

			return;
		}

		const grow = () => {
			settled = painted;
			frames += 1;
		};

		const frame = requestAnimationFrame(grow);
		const timer = setTimeout(grow, 50);

		return () => {
			cancelAnimationFrame(frame);
			clearTimeout(timer);
		};
	});

	return {
		/** Whether the row with this key is drawn yet. */
		has: (key: string) => painted.has(key)
	};
}
