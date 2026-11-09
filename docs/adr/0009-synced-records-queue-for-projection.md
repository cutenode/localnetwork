# Synced records queue for projection in the transaction that stores them

A sync pass stores a repo's records and moves its cursor in one transaction, then projects each record into the tables pages read. Projecting can need other services: the directory for a booking inquiry or a press kit request. So it can fail after the cursor has moved, when neither the oplog nor a full download would bring the record back.

The transaction also writes a row to `pending_projection` for every change, and a projection that works clears its row. One that throws keeps it with a backoff, and later passes of the space project the record as it stands by then, or as a delete if it's gone. A server that stops between the two steps leaves the rows behind and the next pass runs them. A record that can never project stays there, retried hourly and shown on the system page, until a code change lets it through.

Projections can run more than once now, on a throw or after a crash. They were already written to allow it, since a sync can read a record twice, and notifications dedupe on their key.

## Considered options

- Projecting inside the transaction: a failure rolls everything back and the next pass starts over, but the transaction would stay open across directory calls, and one bad record would stop its whole repo from syncing.
- Moving the cursor only after every projection worked: the same stop on one bad record, with the oplog read again each pass.
- A `projected` flag on `space_record`: no new table, but a deleted record has no row to carry it.
- Writing the row only when a projection throws: fewer writes, but a deploy between storing and projecting still loses the record.
