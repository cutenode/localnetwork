#!/usr/bin/env bash
# The organization's projects as code. Each file in .github/projects/ is one project (its views,
# which issues belong on it, and a field set from their labels), and .github/project.json holds
# what they share: the Status options, the org issue fields and the issue types labels stand for.
#   tools/project.sh apply   updates every project from those files, then syncs; safe to run again
#   tools/project.sh sync    adds each open issue to the projects whose scope it's in and takes it
#                            off the ones it's left, fills in each project's field from its
#                            labels, and keeps its Status the same on all of them (the newest
#                            wins, then the rules in tools/project/plan.jq move it along)
#   tools/project.sh types   gives each open issue without a type the one its labels stand for
# apply and sync need a token with the project scope (gh auth refresh -s project); types needs
# one that can edit issues. What the API can't set (views' grouping, sorting and slicing, and the
# built-in workflows) is listed in .github/NEW_PROJECT.md.
# shellcheck disable=SC2016 # the $s in single quotes are GraphQL's and jq's variables
set -euo pipefail
cd "$(dirname "$0")/.."

shared=.github/project.json
repo=${GITHUB_REPOSITORY:-$(gh repo view --json nameWithOwner --jq .nameWithOwner)}
owner=${repo%%/*}
boards=$(find .github/projects -name '*.json' ! -name schema.json | sort)
planner=(jq -L tools/project)

# gql QUERY VARIABLES [JQ]: one GraphQL call, with variables as JSON so lists go through as they are.
gql() {
  jq -n --arg query "$1" --argjson variables "$2" '{$query, $variables}' | gh api graphql --input - ${3:+--jq "$3"}
}

# field_options BOARD: the options for the board's field, one per label it takes (a tool's label,
# or an area's under the field's prefix, without it), coloured by the nearest of the colours
# projects offer, then the field's "other" option if it has one.
field_options() {
  local prefix
  prefix=$(jq -r '.field.prefix // empty' "$1")
  awk '
    /^- name: / { name = substr($0, 9); gsub(/"/, "", name) }
    /^  color: / { color = substr($0, 10); gsub(/"/, "", color) }
    /^  description: / { print name "\t" color "\t" substr($0, 16) }
  ' .github/labels.yml |
    while IFS=$'\t' read -r label color description; do
      if [ -n "$prefix" ]; then
        [[ $label == "$prefix"* ]] || continue
        label=${label#"$prefix"}
      else
        if [[ $label == *" "* ]] || ! { [ -d "apps/$label" ] || [ -d "packages/$label" ] || [ -d "$label" ]; }; then
          continue
        fi
      fi
      jq -n --arg name "$label" --arg hex "$color" --arg description "$description" '
        def byte: ascii_downcase | explode | map(if . >= 97 then . - 87 else . - 48 end) | .[0] * 16 + .[1];
        ($hex | [.[0:2], .[2:4], .[4:6]] | map(byte)) as [$r, $g, $b]
        | ([$r, $g, $b] | max) as $max | ($max - ([$r, $g, $b] | min)) as $d
        | (if $d == 0 then null
          elif $max == $r then 60 * (($g - $b) / $d)
          elif $max == $g then 60 * (($b - $r) / $d + 2)
          else 60 * (($r - $g) / $d + 4) end) as $h
        | (if $h == null then $h else ($h + 360) % 360 end) as $hue
        | {$name, $description, color: (
            if $hue == null then "GRAY"
            elif $hue < 15 or $hue >= 345 then "RED"
            elif $hue < 45 then "ORANGE"
            elif $hue < 70 then "YELLOW"
            elif $hue < 165 then "GREEN"
            elif $hue < 255 then "BLUE"
            elif $hue < 290 then "PURPLE"
            else "PINK" end)}'
    done | jq -s --slurpfile board "$1" '. + [$board[0].field.other // empty]'
}

state() {
  gql 'query($owner: String!, $number: Int!) {
    organization(login: $owner) {
      projectV2(number: $number) {
        id title shortDescription readme
        fields(first: 50) { nodes {
          ... on ProjectV2FieldCommon { id name }
          ... on ProjectV2SingleSelectField { options { id name color description } }
        } }
        views(first: 50) { nodes { id name layout filter fields(first: 50) { nodes { ... on ProjectV2FieldCommon { id } } } } }
      }
    }
  }' "$(jq -n --arg owner "$owner" --argjson number "$1" '{$owner, $number}')" .data.organization.projectV2
}

# field_id NAME: the id of the field with that name in $state.
field_id() {
  jq -r --arg name "$1" '.fields.nodes[] | select(.name == $name) | .id' <<<"$state"
}

# options FIELD_ID WANTED: makes a single select's options WANTED, in order, keeping the ids (and so
# the values items have, and the built-in workflows that set them) of options whose names stay,
# ignoring case.
options() {
  local current
  current=$(jq -c --arg id "$1" '.fields.nodes[] | select(.id == $id) | .options' <<<"$state")
  if [ "$(jq -c 'map({name, color, description})' <<<"$current")" = "$(jq -c 'map({name, color, description})' <<<"$2")" ]; then
    return
  fi
  gql 'mutation($fieldId: ID!, $options: [ProjectV2SingleSelectFieldOptionInput!]) {
    updateProjectV2Field(input: {fieldId: $fieldId, singleSelectOptions: $options}) { clientMutationId }
  }' "$(jq -n --arg fieldId "$1" --argjson current "$current" --argjson wanted "$2" '
    {$fieldId, options: [$wanted[] | . as $o | . + ([$current[] | select(.name | ascii_downcase == ($o.name | ascii_downcase)) | {id}][0] // {})]}')" >/dev/null
  echo "$title: $(jq -r --arg id "$1" '.fields.nodes[] | select(.id == $id) | .name' <<<"$state") options updated"
}

# apply_board FILE: makes the project FILE names match it.
apply_board() {
  local board=$1 n id title field view issue_fields
  n=$(jq .number "$board")
  title=$(jq -r .title "$board")
  state=$(state "$n") || { echo "::error::$owner has no project $n for $board: create it with gh project create and put its number there" >&2; exit 1; }
  id=$(jq -r .id <<<"$state")

  if [ "$(jq -c '[.title, .shortDescription, .readme]' <<<"$state")" != "$(jq -c '[.title, .description, .readme]' "$board")" ]; then
    gql 'mutation($projectId: ID!, $title: String, $shortDescription: String, $readme: String) {
      updateProjectV2(input: {projectId: $projectId, title: $title, shortDescription: $shortDescription, readme: $readme}) { clientMutationId }
    }' "$(jq --arg projectId "$id" '{$projectId, title, shortDescription: .description, readme}' "$board")" >/dev/null
    echo "$title: title, description and readme updated"
  fi

  options "$(field_id Status)" "$(jq -c .status "$shared")"

  field=$(jq -r '.field.name // empty' "$board")
  if [ -n "$field" ]; then
    if [ -z "$(field_id "$field")" ]; then
      gql 'mutation($projectId: ID!, $name: String!, $options: [ProjectV2SingleSelectFieldOptionInput!]) {
        createProjectV2Field(input: {projectId: $projectId, dataType: SINGLE_SELECT, name: $name, singleSelectOptions: $options}) { clientMutationId }
      }' "$(jq -n --arg projectId "$id" --arg name "$field" --argjson options "$(field_options "$board")" '{$projectId, $name, $options}')" >/dev/null
      echo "$title: $field created"
    else
      options "$(field_id "$field")" "$(field_options "$board")"
    fi
  fi

  # The org's issue fields are only looked up for a project that lacks one, so the workflow's token
  # doesn't need the permission to read them once every project has them.
  jq -r '.issueFields[]' "$shared" | while read -r issue_field; do
    if [ -n "$(field_id "$issue_field")" ]; then
      continue
    fi
    issue_fields=${issue_fields:-$(gql 'query($owner: String!) {
      organization(login: $owner) { issueFields(first: 25) { nodes {
        ... on IssueFieldSingleSelect { id name }
        ... on IssueFieldDate { id name }
        ... on IssueFieldText { id name }
        ... on IssueFieldNumber { id name }
      } } }
    }' "$(jq -n --arg owner "$owner" '{$owner}')" .data.organization.issueFields.nodes)}
    gql 'mutation($projectId: ID!, $issueFieldId: ID!) {
      createProjectV2IssueField(input: {projectId: $projectId, issueFieldId: $issueFieldId}) { clientMutationId }
    }' "$(jq -n --arg projectId "$id" --argjson fields "$issue_fields" --arg name "$issue_field" '
      {$projectId, issueFieldId: ($fields[] | select(.name == $name) | .id)}')" >/dev/null
    echo "$title: $issue_field added"
  done

  state=$(state "$n")
  # A new project starts with a table called "View 1": it becomes the first view.
  view=$(jq -r '.views[0].name' "$board")
  if jq -e --arg name "$view" '.views.nodes | (any(.name == "View 1") and all(.name != $name))' <<<"$state" >/dev/null; then
    gql 'mutation($viewId: ID!, $name: String) { updateProjectV2View(input: {viewId: $viewId, name: $name}) { clientMutationId } }' \
      "$(jq --arg name "$view" '{viewId: (.views.nodes[] | select(.name == "View 1") | .id), $name}' <<<"$state")" >/dev/null
    state=$(state "$n")
  fi
  jq -c '.views[]' "$board" | while read -r view; do
    local wanted current
    # The view as updateProjectV2View takes it, with field names turned into ids. A view without
    # fields (roadmaps can't have them) keeps the ones it has.
    wanted=$(jq -c --argjson state "$state" '{name, layout, filter} + if .fields then {configuration: {visibleFieldIds: [
      .fields[] as $f | ($state.fields.nodes | map(select(.name == $f)) | .[0].id // error("no field named \($f)"))
    ] | sort}} else {} end' <<<"$view")
    current=$(jq -c --argjson wanted "$wanted" '
      .views.nodes | map(select(.name == $wanted.name)) | .[0] // empty
      | {id, value: ({name, layout, filter: (.filter // "")}
        + if $wanted.configuration then {configuration: {visibleFieldIds: [.fields.nodes[].id] | sort}} else {} end)}' <<<"$state")
    if [ -z "$current" ]; then
      current=$(gql 'mutation($projectId: ID!, $name: String!, $layout: ProjectV2ViewLayout!) {
        createProjectV2View(input: {projectId: $projectId, name: $name, layout: $layout}) { projectV2View { id } }
      }' "$(jq --arg projectId "$id" '{$projectId, name, layout}' <<<"$wanted")" .data.createProjectV2View.projectV2View.id)
      current=$(jq -c --arg id "$current" '{$id, value: {name, layout}}' <<<"$wanted")
    fi
    if [ "$(jq -c .value <<<"$current")" = "$wanted" ]; then
      continue
    fi
    # Only what differs: GitHub fails on a roadmap view sent its own layout again.
    gql 'mutation($viewId: ID!, $name: String, $layout: ProjectV2ViewLayout, $filter: String, $configuration: ProjectV2ViewConfigurationInput) {
      updateProjectV2View(input: {viewId: $viewId, name: $name, layout: $layout, filter: $filter, configuration: $configuration}) { clientMutationId }
    }' "$(jq --argjson current "$current" '
      with_entries(select(.value != $current.value[.key])) + {viewId: $current.id}' <<<"$wanted")" >/dev/null
    echo "$title: $(jq -r .name <<<"$wanted") view updated"
  done
}

# The open issues, as tools/project/plan.jq takes them. Priority is the org issue field.
issues() {
  gh api graphql --paginate -f owner="$owner" -f name="${repo#*/}" -f query='
    query($owner: String!, $name: String!, $endCursor: String) {
      repository(owner: $owner, name: $name) { issues(states: OPEN, first: 100, after: $endCursor) {
        pageInfo { hasNextPage endCursor }
        nodes {
          id number milestone { title } assignees { totalCount } issueType { name }
          labels(first: 50) { nodes { name } }
          closedByPullRequestsReferences(first: 10, includeClosedPrs: false) { nodes { state } }
          issueFieldValues(first: 20) { nodes { ... on IssueFieldSingleSelectValue { name field { ... on IssueFieldSingleSelect { name } } } } }
        }
      } }
    }' --jq '.data.repository.issues.nodes[]' | jq -s 'map({
      id, number, milestone: .milestone.title, assigned: (.assignees.totalCount > 0), type: .issueType.name,
      labels: [.labels.nodes[].name],
      linked: (.closedByPullRequestsReferences.nodes | any(.state == "OPEN")),
      priority: (first(.issueFieldValues.nodes[] | select(.field.name? == "Priority") | .name) // null)
    })'
}

# board FILE: the project FILE names, as tools/project/plan.jq takes it. Items for other
# repositories' issues, and pull requests and drafts, have no issue, so they're left alone.
board() {
  local n field items
  n=$(jq .number "$1")
  field=$(jq -r '.field.name // "Status"' "$1")
  state=$(state "$n")
  items=$(gh api graphql --paginate -f owner="$owner" -F number="$n" -f field="$field" -f query='
    query($owner: String!, $number: Int!, $field: String!, $endCursor: String) {
      organization(login: $owner) { projectV2(number: $number) { items(first: 100, after: $endCursor) {
        pageInfo { hasNextPage endCursor }
        nodes {
          id content { ... on Issue { id number repository { nameWithOwner } labels(first: 50) { nodes { name } } } }
          status: fieldValueByName(name: "Status") { ... on ProjectV2ItemFieldSingleSelectValue { name updatedAt creator { login } } }
          value: fieldValueByName(name: $field) { ... on ProjectV2ItemFieldSingleSelectValue { name } }
        }
      } } }
    }' --jq '.data.organization.projectV2.items.nodes[]' | jq -s --arg repo "$repo" 'map(.status as $s | {
      id, issue: (if .content.repository.nameWithOwner == $repo then .content.id else null end),
      number: .content.number, labels: [.content.labels.nodes[]?.name], value: .value.name,
      status: (if $s.name then {name: $s.name, updatedAt: $s.updatedAt, auto: ($s.creator.login == "github-project-automation")} else null end)
    })')
  jq -n --slurpfile board "$1" --argjson state "$state" --argjson items "$items" '$board[0] as $b | {
    number: $b.number, project: $state.id, scope: ($b.scope // []),
    status: ($state.fields.nodes[] | select(.name == "Status") | {id, options: [.options[] | {id, name}]}),
    field: (if $b.field then first($state.fields.nodes[] | select(.name == $b.field.name))
      // error("project \($b.number) has no \($b.field.name) field: run tools/project.sh apply")
      | {id, prefix: $b.field.prefix, other: $b.field.other, options: [.options[] | {id, name}]} else null end),
    $items
  }'
}

sync() {
  local all plan
  all=$(for f in $boards; do board "$f"; done | jq -s .)
  plan=$(issues | "${planner[@]}" -c --argjson boards "$all" 'include "plan"; plan($boards)')
  [ -n "$plan" ] || return 0
  while read -r change; do
    local n item number
    n=$(jq .board <<<"$change")
    number=$(jq .number <<<"$change")
    if item=$(jq -er '.delete // empty' <<<"$change"); then
      gh project item-delete "$n" --owner "$owner" --id "$item" >/dev/null
      echo "#$number taken off project $n"
      continue
    fi
    item=$(jq -r '.item // empty' <<<"$change")
    if [ -z "$item" ]; then
      item=$(gh project item-add "$n" --owner "$owner" --url "https://github.com/$repo/issues/$number" --format json --jq .id)
    fi
    jq -r '.set[] | "\(.field) \(.option)"' <<<"$change" | while read -r field option; do
      gh project item-edit --id "$item" --project-id "$(jq -r .project <<<"$change")" --field-id "$field" --single-select-option-id "$option" >/dev/null
    done
    echo "#$number synced on project $n"
  done <<<"$plan"
}

types() {
  issues | "${planner[@]}" -r --argjson types "$(jq .types "$shared")" \
    'include "plan"; .[] | type_for($types) as $type | select($type) | "\(.number) \($type)"' |
    while read -r number type; do
      gh issue edit "$number" --repo "$repo" --type "$type" >/dev/null
      echo "#$number is a $type"
    done
}

case "${1:-}" in
  apply)
    for f in $boards; do apply_board "$f"; done
    sync
    ;;
  sync) sync ;;
  types) types ;;
  *)
    echo "usage: tools/project.sh apply | sync | types" >&2
    exit 2
    ;;
esac
