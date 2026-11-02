// How tall a panel's frame (PanelShell, PanelGhost) may be.
//
// The frame owns the popup's one scroll area, so it must never be taller than
// the popup: past that, the whole popup scrolls and the header goes with it.
// 580px fits under Chrome's 600px ceiling at 100% zoom on a screen with room
// for it. Zoomed in, or on a short screen, the popup gets less.
//
// `100vh` cannot say how much less. A popup is sized from its content, so a
// frame sized from the popup only ever stays as small as the popup opened.
// The one moment the popup's height is Chrome's limit rather than the
// content's is when the content still overflows it once Chrome has stopped
// resizing it, so that is when the cap comes down to it. Not before: while a
// panel fills in, the content outgrows the popup for a frame or two before
// Chrome catches up, and a cap taken then would hold the popup that small.

/** What fits under Chrome's ceiling at 100% zoom. */
const CEILING = 580;

/** How long the popup has to stay one size before its height is taken as all the room there is. */
const SETTLED = 200;

class FrameHeight {
	#max = $state(CEILING);

	#started = false;

	/** The frame's max-height in CSS pixels. */
	get max() {
		return this.#max;
	}

	/** Starts watching for the popup running out of room. Safe to call more than once. */
	start() {
		if (this.#started || typeof ResizeObserver === 'undefined') return;
		this.#started = true;

		let timer: ReturnType<typeof setTimeout> | undefined;

		const fit = () => {
			if (document.documentElement.scrollHeight > innerHeight) this.#max = innerHeight;
		};

		const settle = () => {
			clearTimeout(timer);
			timer = setTimeout(fit, SETTLED);
		};

		// The content growing past the popup, and the popup itself being resized as Chrome fits it.
		new ResizeObserver(settle).observe(document.documentElement);
		addEventListener('resize', settle);
	}
}

export const frameHeight = new FrameHeight();
