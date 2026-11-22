# The sign-in page

`/login` says why someone is there from where they'll go next ("to follow Neon Tide", "to book Neon Tide"), takes one handle, and suggests up to five accounts as they type. The suggestions come from Bluesky's AppView (`app.bsky.actor.searchActorsTypeahead`) through `/api/actors`, so visitors' keystrokes never leave this server, repeats are answered from a ten-minute in-memory cache, and the CSP keeps `connect-src 'self'`. HappyView can't answer them: it keeps every Bluesky profile, but reads them quickly only by account, not by name.
