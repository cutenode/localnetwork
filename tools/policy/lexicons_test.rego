package lexicons_test

import data.fixtures
import data.lexicons

test_fixture_passes if {
	count(lexicons.deny) == 0 with input as fixtures.repo
}

test_id_differs_from_path if {
	broken := {"id": "computer.localnetwork.tool.other"}
	"lexicons/computer/localnetwork/tool/record.json: its id should be computer.localnetwork.tool.record, the NSID its path spells" in lexicons.deny with input as fixtures.with_file("lexicons/computer/localnetwork/tool/record.json", broken)
}

test_document_without_id if {
	"lexicons/computer/localnetwork/tool/record.json: its id should be computer.localnetwork.tool.record, the NSID its path spells" in lexicons.deny with input as fixtures.with_file("lexicons/computer/localnetwork/tool/record.json", {})
}

test_group_of_no_tool if {
	repo := array.concat(fixtures.repo, [{"path": "lexicons/computer/localnetwork/gig/offer.json", "contents": {"id": "computer.localnetwork.gig.offer"}}])
	"lexicons/computer/localnetwork/gig/offer.json: put a tool's records under computer.localnetwork.<tool>, or add \"gig\" to the shared groups in tools/policy/lexicons.rego" in lexicons.deny with input as repo
}

test_vendored_third_party_lexicon if {
	repo := array.concat(fixtures.repo, [{"path": "lexicons/community/lexicon/calendar/rsvp.json", "contents": {"id": "community.lexicon.calendar.rsvp"}}])
	"lexicons/community/lexicon/calendar/rsvp.json: other authorities' lexicons come from the network, not the repo: add the NSID to the lexicon manifest instead" in lexicons.deny with input as repo
}

test_string_without_a_limit if {
	record := {"id": "computer.localnetwork.tool.record", "defs": {"main": {"type": "record", "record": {"type": "object", "properties": {
		"handle": {"type": "string", "maxLength": 253},
		"name": {"type": "string", "maxGraphemes": 64},
		"createdAt": {"type": "string", "format": "datetime"},
		"tags": {"type": "array", "items": {"type": "string"}},
	}}}}}
	denied := lexicons.deny with input as fixtures.with_file("lexicons/computer/localnetwork/tool/record.json", record)
	denied == {"lexicons/computer/localnetwork/tool/record.json: defs.main.record.properties.tags.items needs a maxLength (and a maxGraphemes, for text people write) or a format"}
}

test_other_authorities_strings_are_theirs if {
	address := {"id": "community.lexicon.location.address", "defs": {"main": {"type": "string"}}}
	count(lexicons.deny) == 0 with input as fixtures.with_file("lexicons/community/lexicon/location/address.json", address)
}
