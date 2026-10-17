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
