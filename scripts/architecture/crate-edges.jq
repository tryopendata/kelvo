# The named graph is an allowlist, including dev edges; unknown members fail.
. as $metadata |
[$metadata.packages[] | .name as $from |
  if ($allowed[0] | has($from) | not) then "unlisted workspace member: \($from)"
  else .dependencies[] | select(.path != null) |
    (.kind // "normal") as $kind | .name as $to |
    select(($allowed[0][$from][$kind] // [] | index($to)) == null) |
    "forbidden \($kind) edge: \($from) -> \($to)"
  end
] as $violations |
if ($violations | length) == 0 then true
else error($violations | join("\n")) end
