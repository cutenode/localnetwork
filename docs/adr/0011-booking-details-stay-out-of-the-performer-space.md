# Booking details stay out of the performer space

A space credential covers the whole space, so anyone in a performer's space could read whatever it holds with another atproto client, and the site would never know they had. That includes hidden members and people whose capabilities don't touch bookings. A performer's booking details (email, phone, rates, rider) therefore get a space of their own, keyed by the performer space's key, with the root as its only member and writer. The people who hold `bookings.answer` read them through the app, which checks the capability on every read; staff read them through the app too, not as members.

This amends [0003](0003-profiles-in-repos-performers-in-spaces.md), which had everything a performer shares live in its space.

## Considered options

- Details in the performer space: every member reads them, with no way to tell who did.
- Details in the root's industry space, as projects keep them: staff read every performer's details from any client, and one space holds them all.
- A booking details space whose members are the people answering bookings: membership would have to follow capability changes, and a member still reads without a trace.
