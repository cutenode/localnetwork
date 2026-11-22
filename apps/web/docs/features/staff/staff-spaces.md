# Staff spaces

Staff are members of the audit, moderation, staff and industry spaces. Every 5 minutes, at startup, and on the System page's bootstrap, the app compares those member lists with the roles in the root's repo and takes out anyone without one, which finishes a removal that failed and drops an account taken out of `ADMIN_DIDS`. The audit log marks a removal that left someone in a space as failed, then records when they're out, and `/admin/staff` lists who's left until then. A space credential already issued stays valid for up to 10 minutes after someone leaves.
