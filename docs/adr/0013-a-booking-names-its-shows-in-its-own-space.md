# A booking names its shows in its own space

A booking thread links two shows: the promoter's planner show it started from, and the public show a manager announced once it was confirmed. Both links are written into the booking space, so only the thread's participants can read them, and neither show names the booking. A public show that pointed at a booking space would tell anyone reading the show that a private thread exists and who it's under.

The inquiry names its planner show (`plannerShow`), since the promoter writes it into their own repo there. The server reads the show back from the promoter's own planner before naming it, and the thread page only links one that sits in the promoter's own planner space, so a rewritten inquiry can't point anyone at another planner. The planner is the promoter's private space, so only they get the link.

The announced show is a status record the root writes into its own repo in the space (`announcedShow`), keeping the status `confirmed`, once the app has checked the manager is in a confirmed thread with the project the show is for. The root's status records never move a thread or show in its timeline, as with the record opening it ([0008](0008-booking-status-follows-the-rules-not-record-dates.md)), and the thread only trusts the root's for this, so a participant can't name a show that wasn't announced from it.

## Considered options

- A `booking` field on `computer.localnetwork.show`: one write, but every public show from a booking would point at a private space.
- A new booking record type for the announced show: says what it is, but adds a collection to the space type, the permission set and the sync for one field a status record carries.
- The manager writing the status record into their own repo: closer to who did it, but any participant could then write one naming any show, and the thread would have to check the show's author and project on every read.
