# Booking status follows the rules, not record dates

Each side of a booking writes its status records into its own repo, so a modified client can write any status with any date, and a sync reads repos in no particular order. A status record therefore only proposes a change. The thread keeps its own status and applies a record when the thread's rules allow that side the move from where it stands now. A record that doesn't fit waits in the index, out of the thread's timeline, and is tried again whenever another record joins the thread, so a confirmation read before the reply that allows it still lands, and a thread that's settled stays settled. Once read, a status record is fixed: rewriting or deleting it changes nothing.

This means two reads of the same records in different orders can pass through different statuses. Any order of a valid history ends on the same one, because statuses only move forward and each move has one possible end. Rivals that really were simultaneous, like a decline and a withdrawal, go to whichever is read first.

A rebuilt index has no memory of what it refused, so it judges the records in the repos afresh. A record planted after a thread settled can win there. Protecting against that would take the root countersigning every status, and a rebuild is rare enough, and a wrong status cheap enough to spot, that this doesn't pay for an extra write on every change.

## Considered options

- Replaying every status record in date order on each change: gives the same answer in any read order, but the author picks the date, so a record backdated to before a decline overturns it. Managers who've left have no role to replay either, so their past moves would unwind.
- Dropping a record that doesn't fit when it's read: simple, but one read out of order loses a valid change and strands the thread behind it.
- Having the root countersign each applied status in its own repo: survives a rebuild, at the cost of a root write on every change, including those a sync hears of, and a status that can't settle while the root's PDS is down.
