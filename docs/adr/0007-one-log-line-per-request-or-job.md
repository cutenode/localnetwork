# One log line per request or job

The website writes one line for each request and each background job, using evlog. Whatever the code notes along the way joins that line under `notes`, each note with its own fields, because services run under both requests and jobs and their `did` or `status` aren't the request's. Services keep a plain logger and never ask which of the two they're under: an `AsyncLocalStorage` finds the line. A job that noted nothing writes no line, since most runs of the 15-second jobs have nothing to say.

A line's `message` lists every note's message, because Railway searches `message`, and its `level` is the worst of its notes and, for a request, its status. Lines go to stdout whatever their level: Railway files stderr under `error`, and evlog's own console output sends warnings there. A note that comes after its line was written, like a streamed page's failed promise, gets a line of its own with the request's id, as does a note past the 20 a line holds.

## Considered options

- evlog's SvelteKit hook with `useLogger()`: it throws outside a request, so jobs and startup couldn't share the services' code, and a note after the line is dropped.
- evlog's `log.warn(message, fields)` on the line itself: it merges a note's fields into the line, so the second `space` replaces the first and a note's `status` replaces the response's.
- A line per note, as pino wrote them: nothing joins what one request did.
