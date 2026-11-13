// The content script on the gate sites done on their own page (Droploud, Pump
// Your Sound, Laylo: `how: 'page'` in src/tools/third-party-downloads/sites/).
//
// It runs on every page of those sites and does nothing unless the background
// worker says this tab is one it opened for a gate (GATE_PAGE_HELLO). Then it
// helps the person do the gate themselves:
//
//   a small bar in the corner says what to press, which gate of how many this
//   is, and offers the next one;
//   what to press is outlined, and scrolled to once;
//   the email field, on a page that has one, is filled with the address from
//   settings while it is empty, and only once, so an address the person clears
//   or changes stays theirs;
//   on a sign-up page, the person's submit of the form is reported back, so the
//   Downloader's row can say they signed up.
//
// It never presses anything, ticks a consent box or submits a form. The file a
// gate unlocks is the page's own download, which the worker files into the
// site's folder; the worker then says so here (GATE_PAGE_SAVED), and this
// marks the track, through the same store content.ts marks with.
//
// Depends on the TbxDownloaded global from downloaded-store.ts, loaded first.

(() => {
	type GatePageHelp = Tbx.GatePageHelp;

	type GatePageRequest = Tbx.GatePageRequest;

	type GatePageSaved = Tbx.GatePageSaved;

	const STEP = 'data-tbx-step';

	const ask = async (message: GatePageRequest): Promise<GatePageHelp | null> => {
		try {
			return (await chrome.runtime.sendMessage(message)) ?? null;
		} catch (_e) {
			// No worker to ask: the extension was reloaded under this page.
			return null;
		}
	};

	// The bar is in a shadow root, so the page's styles and ours stay apart.
	const STYLE = `
		:host { all: initial; }
		.card {
			--ink: #111113; --paper: #ffffff; --soft: #5f5f68; --line: #e2e2e6;
			position: fixed; right: 16px; bottom: 16px; z-index: 2147483647;
			box-sizing: border-box; width: min(320px, calc(100vw - 32px));
			padding: 12px; border: 1px solid var(--line); border-radius: 12px;
			background: var(--paper); color: var(--ink);
			box-shadow: 0 8px 24px rgb(0 0 0 / 0.18);
			font: 13px/1.45 system-ui, -apple-system, 'Segoe UI', Roboto, sans-serif;
		}
		@media (prefers-color-scheme: dark) {
			.card { --ink: #f4f4f5; --paper: #161618; --soft: #a1a1aa; --line: #2e2e33; }
		}
		.top { display: flex; align-items: center; gap: 8px; }
		.name { font-weight: 600; }
		.count { color: var(--soft); font-variant-numeric: tabular-nums; }
		.close {
			margin-left: auto; width: 24px; height: 24px; padding: 0; border: 0; border-radius: 6px;
			background: transparent; color: var(--soft); font: inherit; font-size: 16px; cursor: pointer;
		}
		.close:hover { color: var(--ink); }
		p { margin: 6px 0 0; }
		.next {
			margin-top: 10px; padding: 5px 12px; border: 0; border-radius: 8px;
			background: var(--ink); color: var(--paper); font: inherit; font-weight: 600; cursor: pointer;
		}
		.next[hidden] { display: none; }
		button:focus-visible { outline: 2px solid #ff5500; outline-offset: 2px; }
	`;

	// Outlines what to press. On the page, not in the shadow root, so it is a
	// rule of its own, scoped to the attribute this script puts on.
	const OUTLINE = `[${STEP}] { outline: 2px solid #ff5500 !important; outline-offset: 4px !important; }`;

	const start = async () => {
		const help = await ask({ type: 'GATE_PAGE_HELLO' });

		if (!help) return;

		const host = document.createElement('div');
		const root = host.attachShadow({ mode: 'open' });
		root.innerHTML = `<style>${STYLE}</style>
			<div class="card" role="region" aria-label="localtoolbox">
				<div class="top">
					<span class="name">localtoolbox</span>
					<span class="count"></span>
					<button type="button" class="close" aria-label="close">×</button>
				</div>
				<p role="status"></p>
				<button type="button" class="next" hidden>next</button>
			</div>`;
		const say = root.querySelector('p')!;
		const next = root.querySelector<HTMLButtonElement>('.next')!;
		const more = help.at < help.of;

		root.querySelector('.count')!.textContent = help.of > 1 ? `${help.at} of ${help.of}` : '';
		say.textContent = help.say;
		next.textContent = 'skip';
		next.hidden = !more;

		const outline = document.createElement('style');
		outline.textContent = OUTLINE;
		document.head.append(outline);
		document.body.append(host);

		const filled = new WeakSet<Element>();
		let scrolled = false;

		// The page draws itself after this script runs, and redraws as the steps
		// go by, so this is redone on every change rather than once.
		const tend = () => {
			for (const step of document.querySelectorAll(help.steps)) {
				if (step.hasAttribute(STEP)) continue;
				step.setAttribute(STEP, '');

				if (!scrolled) {
					scrolled = true;
					step.scrollIntoView({ block: 'center' });
				}
			}

			if (!help.email || !help.emailField) return;

			for (const field of document.querySelectorAll<HTMLInputElement>(help.emailField)) {
				if (filled.has(field)) continue;
				filled.add(field);

				if (field.value !== '') continue;
				// Set from this script's world, which the page's own value tracking
				// does not see, so the input event below reads to it as typing.
				field.value = help.email;
				field.dispatchEvent(new Event('input', { bubbles: true }));
				field.dispatchEvent(new Event('change', { bubbles: true }));
			}
		};

		const watch = new MutationObserver(tend);
		tend();
		watch.observe(document.documentElement, { childList: true, subtree: true });

		const end = () => {
			watch.disconnect();
			host.remove();
			outline.remove();

			for (const step of document.querySelectorAll(`[${STEP}]`)) step.removeAttribute(STEP);
		};

		root.querySelector('.close')!.addEventListener('click', () => {
			void ask({ type: 'GATE_PAGE_STOP' });
			end();
		});

		// Only shown while there is a next gate, which the worker opens in this tab.
		next.addEventListener('click', () => void ask({ type: 'GATE_PAGE_NEXT' }));

		if (help.signup) {
			document.addEventListener(
				'submit',
				(event) => {
					if (!(event.target instanceof Element) || !event.target.matches(help.signup)) return;
					void ask({ type: 'GATE_PAGE_SIGNED_UP' });
					say.textContent = 'RSVP sent. the link comes by mail.';
					next.textContent = 'next';
				},
				true
			);
		}

		chrome.runtime.onMessage.addListener((message: GatePageSaved) => {
			if (message?.type !== 'GATE_PAGE_SAVED') return;

			if (message.mark) void TbxDownloaded.set(message.mark, true).catch(() => {});
			say.textContent = `saved to ${message.path}.${message.more ? ' the next one opens in a moment.' : ''}`;
			next.textContent = 'next';
			next.hidden = !message.more;
		});
	};

	void start();
})();
