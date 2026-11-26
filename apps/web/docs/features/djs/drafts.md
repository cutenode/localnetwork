# Drafts

The project form and the booking inquiry keep unsent changes as a draft in the browser (`localStorage`, per account), so a reload or a trip to another page doesn't lose them. Files have to be picked again, and the draft goes once the form is sent, discarded, or back to its saved values, or its account signs out (the sign-out response sets an `ln_signed_out` cookie naming the account, and the next page load drops its drafts, so it works even if Sign out was pressed before the page's scripts ran).
