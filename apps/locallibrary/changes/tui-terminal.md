### Added

- The player asks the terminal whether it draws 24-bit colour before going by the environment, and
  knows Terminal.app does from macOS 26 on. It gives up after 100 ms at the most.
- 16-colour terminals get a palette of their own, on ANSI 0-15. Text is the terminal's foreground,
  and the selection and chips are in reverse video.
- `LOCALLIBRARY_COLOR=truecolor|256|16` and the palette's "16 colours" and the like, for a terminal
  that says the wrong thing of itself. The palette's choice is kept, and goes with `settings export`.
- Icons in three sets, Nerd Font, Unicode and ASCII, chosen by terminal, by the palette or by
  `LOCALLIBRARY_ICONS=nerd|unicode|ascii`. With ASCII, nothing else on screen is anything but ASCII.
- A theme can set `ui.rule`, `ui.ink`, `ui.idle`, `ui.marked` and `ui.mixes`.

### Changed

- The player reads the terminal's real background and foreground. A light one gets the light
  palette without `LOCALLIBRARY_THEME`, what's behind rows follows it, and text and markers are
  kept readable on it: 4.5:1 for text, 3:1 for waveforms and markers.
- The light palette is designed, from the shared tokens, rather than worked out from the dark one.
- Hints are easier to read on a dark terminal: `ui.dim` and `ui.faint` are lighter, and rails and
  borders have `ui.rule`.
- Each frame reaches the terminal in one piece, on terminals that support synchronized output.
