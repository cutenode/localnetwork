#!/usr/bin/env bash
# The organization's "Local Network" project as code: .github/project.json holds its Status options,
# org issue fields and views, and the Product field's options are the tools' labels in
# .github/labels.yml (a label named after a folder in apps/, packages/ or the root) plus "other".
#   tools/project.sh apply          creates or updates the project from those files, then syncs it;
#                                   safe to run again
#   tools/project.sh sync [number]  adds every open issue (or just that one) to the project, sets an
#                                   empty Status to the first option, and Product from its labels
#                                   (or, when it has no tool's label and no Product, to "other")
# Both need a token with the project scope (gh auth refresh -s project). What the API can't set
# (views' grouping, sorting and slicing, and the built-in workflows) is listed in CONTRIBUTING.md.
# shellcheck disable=SC2016 # the $s in single quotes are GraphQL's and jq's variables
set -euo pipefail
cd "$(dirname "$0")/.."

config=.github/project.json
repo=${GITHUB_REPOSITORY:-$(gh repo view --json nameWithOwner --jq .nameWithOwner)}
owner=${repo%%/*}
title=$(jq -r .title "$config")
product=$(jq -r .product.name "$config")

# gql QUERY VARIABLES [JQ]: one GraphQL call, with variables as JSON so lists go through as they are.
gql() {
  jq -n --arg query "$1" --argjson variables "$2" '{$query, $variables}' | gh api graphql --input - ${3:+--jq "$3"}
}

# The Product options: each tool's label, coloured by the nearest of the colours projects offer, then
# the catch-all from the config.
products() {
  awk '
    /^- name: / { name = ($0 ~ /^- name: [a-z0-9-]+$/) ? substr($0, 9) : "" }
    /^  color: / { color = substr($0, 10); gsub(/"/, "", color) }
    /^  description: / && name != "" { print name "\t" color "\t" substr($0, 16) }
  ' .github/labels.yml |
    while IFS=$'\t' read -r name color description; do
      [ -d "apps/$name" ] || [ -d "packages/$name" ] || [ -d "$name" ] || continue
      jq -n --arg name "$name" --arg hex "$color" --arg description "$description" '
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
    done | jq -s --argjson other "$(jq .product.other "$config")" '. + [$other]'
}

number() {
  gh project list --owner "$owner" --format json --limit 100 --jq ".projects[] | select(.title == \"$title\") | .number"
}

state() {
  gql 'query($owner: String!, $number: Int!) {
    organization(login: $owner) {
      projectV2(number: $number) {
        id shortDescription readme
        repositories(first: 20) { nodes { nameWithOwner } }
        fields(first: 50) { nodes {
          ... on ProjectV2FieldCommon { id name }
          ... on ProjectV2SingleSelectField { options { id name color description } }
        } }
        views(first: 50) { nodes { id name layout filter fields(first: 50) { nodes { ... on ProjectV2FieldCommon { id } } } } }
      }
    }
  }' "$(jq -n --arg owner "$owner" --argjson number "$1" '{$owner, $number}')" .data.organization.projectV2
}

# options FIELD_ID WANTED: makes a single select's options WANTED, in order, keeping the ids (and so
# the values items have) of options whose names stay.
options() {
  local current
  current=$(jq -c --arg id "$1" '.fields.nodes[] | select(.id == $id) | .options' <<<"$state")
  if [ "$(jq -c 'map({name, color, description})' <<<"$current")" = "$(jq -c 'map({name, color, description})' <<<"$2")" ]; then
    return
  fi
  gql 'mutation($fieldId: ID!, $options: [ProjectV2SingleSelectFieldOptionInput!]) {
    updateProjectV2Field(input: {fieldId: $fieldId, singleSelectOptions: $options}) { clientMutationId }
  }' "$(jq -n --arg fieldId "$1" --argjson current "$current" --argjson wanted "$2" '
    {$fieldId, options: [$wanted[] | . as $o | . + ([$current[] | select(.name == $o.name) | {id}][0] // {})]}')" >/dev/null
  echo "$(jq -r --arg id "$1" '.fields.nodes[] | select(.id == $id) | .name' <<<"$state"): options updated"
}

apply() {
  local n id view
  n=$(number)
  if [ -z "$n" ]; then
    n=$(gh project create --owner "$owner" --title "$title" --format json --jq .number)
    echo "created project $n"
  fi
  state=$(state "$n")
  id=$(jq -r .id <<<"$state")

  if [ "$(jq -c '[.shortDescription, .readme]' <<<"$state")" != "$(jq -c '[.description, .readme]' "$config")" ]; then
    gql 'mutation($projectId: ID!, $shortDescription: String, $readme: String) {
      updateProjectV2(input: {projectId: $projectId, shortDescription: $shortDescription, readme: $readme}) { clientMutationId }
    }' "$(jq --arg projectId "$id" '{$projectId, shortDescription: .description, readme}' "$config")" >/dev/null
    echo "description and readme updated"
  fi

  if ! jq -e --arg repo "$repo" '.repositories.nodes | any(.nameWithOwner == $repo)' <<<"$state" >/dev/null; then
    gh project link "$n" --owner "$owner" --repo "$repo"
  fi

  options "$(jq -r '.fields.nodes[] | select(.name == "Status") | .id' <<<"$state")" "$(jq -c .status "$config")"

  if ! jq -e --arg name "$product" '.fields.nodes | any(.name == $name)' <<<"$state" >/dev/null; then
    gql 'mutation($projectId: ID!, $name: String!, $options: [ProjectV2SingleSelectFieldOptionInput!]) {
      createProjectV2Field(input: {projectId: $projectId, dataType: SINGLE_SELECT, name: $name, singleSelectOptions: $options}) { clientMutationId }
    }' "$(jq -n --arg projectId "$id" --arg name "$product" --argjson options "$(products)" '{$projectId, $name, $options}')" >/dev/null
    echo "$product: created"
  else
    options "$(jq -r --arg name "$product" '.fields.nodes[] | select(.name == $name) | .id' <<<"$state")" "$(products)"
  fi

  local issue_fields
  issue_fields=$(gql 'query($owner: String!) {
    organization(login: $owner) { issueFields(first: 25) { nodes {
      ... on IssueFieldSingleSelect { id name }
      ... on IssueFieldDate { id name }
      ... on IssueFieldText { id name }
      ... on IssueFieldNumber { id name }
    } } }
  }' "$(jq -n --arg owner "$owner" '{$owner}')" .data.organization.issueFields.nodes)
  jq -r '.issueFields[]' "$config" | while read -r name; do
    if jq -e --arg name "$name" '.fields.nodes | any(.name == $name)' <<<"$state" >/dev/null; then
      continue
    fi
    gql 'mutation($projectId: ID!, $issueFieldId: ID!) {
      createProjectV2IssueField(input: {projectId: $projectId, issueFieldId: $issueFieldId}) { clientMutationId }
    }' "$(jq -n --arg projectId "$id" --argjson fields "$issue_fields" --arg name "$name" '
      {$projectId, issueFieldId: ($fields[] | select(.name == $name) | .id)}')" >/dev/null
    echo "$name: added"
  done

  state=$(state "$n")
  # A new project starts with a table called "View 1": it becomes the first view.
  view=$(jq -r '.views[0].name' "$config")
  if jq -e --arg name "$view" '.views.nodes | (any(.name == "View 1") and all(.name != $name))' <<<"$state" >/dev/null; then
    gql 'mutation($viewId: ID!, $name: String) { updateProjectV2View(input: {viewId: $viewId, name: $name}) { clientMutationId } }' \
      "$(jq --arg name "$view" '{viewId: (.views.nodes[] | select(.name == "View 1") | .id), $name}' <<<"$state")" >/dev/null
    state=$(state "$n")
  fi
  jq -c '.views[]' "$config" | while read -r view; do
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
    echo "$(jq -r .name <<<"$wanted") view: updated"
  done

  sync
}

sync() {
  local n state project issues items
  n=$(number)
  [ -n "$n" ] || { echo "::error::$owner has no \"$title\" project: run tools/project.sh apply" >&2; exit 1; }
  state=$(state "$n")
  project=$(jq -r .id <<<"$state")
  if [ -n "${1:-}" ]; then
    issues=$(gh issue view "$1" --repo "$repo" --json id,number,state,labels --jq '[select(.state == "OPEN")]')
  else
    issues=$(gh issue list --repo "$repo" --state open --limit 1000 --json id,number,labels)
  fi
  items=$(gh api graphql --paginate -f owner="$owner" -F number="$n" -f product="$product" -f query='
    query($owner: String!, $number: Int!, $product: String!, $endCursor: String) {
      organization(login: $owner) { projectV2(number: $number) { items(first: 100, after: $endCursor) {
        pageInfo { hasNextPage endCursor }
        nodes {
          id content { ... on Issue { id } }
          status: fieldValueByName(name: "Status") { ... on ProjectV2ItemFieldSingleSelectValue { name } }
          product: fieldValueByName(name: $product) { ... on ProjectV2ItemFieldSingleSelectValue { name } }
        }
      } } }
    }' --jq '.data.organization.projectV2.items.nodes[]' | jq -s .)

  # One line per issue that needs something: its item (empty when it isn't in the project yet), the
  # Status and Product option ids to set (empty to leave them), and its number.
  jq -r --argjson state "$state" --argjson items "$items" --arg product "$product" '
    ($state.fields.nodes | map(select(.name == "Status")) | .[0]) as $status
    | ($state.fields.nodes | map(select(.name == $product)) | .[0]) as $products
    | .[] | .id as $issue
    | ($items | map(select(.content.id == $issue)) | .[0]) as $item
    | [.labels[].name | split(": ")[0]] as $names
    | [$products.options[] | select(.name as $o | $names | index($o))] as $tools
    | (if ($tools | length) > 0 then (if $tools | any(.name == $item.product.name) then "" else $tools[0].id end)
      elif $item.product.name then ""
      else $products.options[-1].id end) as $set_product
    | (if $item.status.name then "" else $status.options[0].id end) as $set_status
    | select($item == null or $set_status != "" or $set_product != "")
    | [($item.id // ""), $set_status, $set_product, .number] | join("|")
  ' <<<"$issues" | while IFS='|' read -r item status tool number; do
    if [ -z "$item" ]; then
      item=$(gh project item-add "$n" --owner "$owner" --url "https://github.com/$repo/issues/$number" --format json --jq .id)
    fi
    if [ -n "$status" ]; then
      gh project item-edit --id "$item" --project-id "$project" --field-id "$(jq -r '.fields.nodes[] | select(.name == "Status") | .id' <<<"$state")" --single-select-option-id "$status" >/dev/null
    fi
    if [ -n "$tool" ]; then
      gh project item-edit --id "$item" --project-id "$project" --field-id "$(jq -r --arg name "$product" '.fields.nodes[] | select(.name == $name) | .id' <<<"$state")" --single-select-option-id "$tool" >/dev/null
    fi
    echo "#$number synced"
  done
}

case "${1:-}" in
  apply) apply ;;
  sync) sync "${2:-}" ;;
  *)
    echo "usage: tools/project.sh apply | sync [issue number]" >&2
    exit 2
    ;;
esac
