-- Runs before HappyView stores an app.userinput.space event. Jetstream sends every userinput.app space, and bug reports
-- only point at Local Network's own (USERINPUT_SPACE, nil when none is configured), so only that one gets in, as
-- written. Its CID is what a report pins.

function handle()
	if uri ~= USERINPUT_SPACE then return nil end
	return true
end
