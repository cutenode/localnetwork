# What tools/project.sh sync and types change, worked out from what they read. No I/O here, so
# tools/project/plan.test can check it with `jq --run-tests`.
#
# An issue: {id, number, labels: [name], milestone, assigned, priority, linked, type}, where linked
# is whether an open pull request closes it.
# A board: {number, project, scope: [label], status: {id, options: [{id, name}]},
#   field: {id, prefix?, other?, options: [{id, name}]} or null, items: [item]}.
# An item: {id, issue, number, labels: [name], status: {name, updatedAt, auto} or null, value},
#   where auto is whether GitHub's built-in workflows set the Status, and value is the field's.

# Whether an issue (or item) belongs on a board with this scope: it has one of the labels, or one
# of their "<label>: <area>" labels. An empty scope takes every issue.
def in_scope($scope):
  ($scope | length) == 0
  or (.labels | any(. as $label | $scope | any(. as $tool | $label == $tool or ($label | startswith($tool + ": ")))));

# The option id to set the board's field to, or null to leave it: the first option one of the
# issue's labels names, unless the field already holds one of those. With none, the field's
# "other" option (its last) goes in only while the field is empty.
def field_option($field; $current):
  (if $field.prefix then [.labels[] | select(startswith($field.prefix)) | ltrimstr($field.prefix)]
  else [.labels[] | split(": ")[0]] end) as $keys
  | [$field.options[] | select(.name as $name | $keys | any(. == $name))] as $matches
  | if ($matches | length) > 0 then
      (if $matches | any(.name == $current) then null else $matches[0].id end)
    elif $current != null or $field.other == null then null
    else $field.options[-1].id end;

# An open issue's Status on every board it's on, from the Status options in order (new, triaged,
# in progress, done) and its Status on each board. The newest one a person (or this script) set
# wins over the defaults GitHub's workflows fill in; then:
# - a done issue that's open again is triaged;
# - a new or triaged issue that an open pull request closes is in progress;
# - a new issue with a Priority, a milestone or an assignee is triaged.
def status($values; $options):
  . as $issue
  | [$values[] | select(.name as $name | $options | any(. == $name))] as $valid
  | ([$valid[] | select(.auto | not)] | max_by(.updatedAt)) // ($valid | max_by(.updatedAt))
  | (.name // $options[0])
  | if . == $options[-1] then $options[1] else . end
  | if (. == $options[0] or . == $options[1]) and $issue.linked then $options[2]
    elif . == $options[0] and ($issue.priority != null or $issue.milestone != null or $issue.assigned) then $options[1]
    else . end;

# One line per change to a board: {board, project, number, issue, item, set: [{field, option}]},
# where a null item means adding the issue first, or {board, project, number, delete: item} for
# an issue that's left the board's scope. Takes the open issues; $boards as above.
def plan($boards):
  . as $issues
  | ($boards[0].status.options | map(.name)) as $options
  | (
    $issues[] as $issue
    | [$boards[] | select(.scope as $scope | $issue | in_scope($scope))
      | {board: ., item: (first(.items[] | select(.issue == $issue.id)) // null)}] as $on
    | ($issue | status([$on[].item.status | select(. != null)]; $options)) as $status
    | $on[] | .board as $board | .item as $item
    | [
      (if $item.status.name != $status then
        {field: $board.status.id, option: (first($board.status.options[] | select(.name == $status) | .id)
          // error("project \($board.number) has no \($status) Status: run tools/project.sh apply"))}
      else empty end),
      (if $board.field then
        ($issue | field_option($board.field; $item.value)) as $option
        | if $option then {field: $board.field.id, $option} else empty end
      else empty end)
    ] as $set
    | select($item == null or ($set | length) > 0)
    | {board: $board.number, project: $board.project, number: $issue.number, issue: $issue.id, item: $item.id, $set}
  ), (
    $boards[] | select(.scope | length > 0) | . as $board
    | .items[] | select(.issue != null and (in_scope($board.scope) | not))
    | {board: $board.number, project: $board.project, number, delete: .id}
  );

# The issue type for an open issue without one, from the first of its labels that names one, or
# null to leave it.
def type_for($types):
  if .type then null
  else first(.labels[] as $label | $types.labels[$label] // empty) // $types.default end;
