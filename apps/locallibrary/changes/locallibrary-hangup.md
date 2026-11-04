### Fixed

- The player exits when its terminal goes away (a window closed, or ttyd or VHS closing the
  terminal it ran in), first letting a drive write under way finish and saving what it was
  showing, as `q` does. It used to spin at full CPU until killed, deaf to SIGHUP and SIGTERM, and
  when it did notice the terminal was gone it crashed as it put the terminal back.
