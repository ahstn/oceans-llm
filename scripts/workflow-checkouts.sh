# shellcheck shell=bash
# Sourced by the CI policy checks; uses only awk so it runs on hosts without ripgrep.
#
# Prints one line per actions/checkout step in a workflow: the ref, path and
# persist-credentials inputs from that step's own `with:` block. Steps must sit at six
# spaces, with `with:` at eight and its inputs at ten, as in the repository's workflows.
# An input in any other layout or outside `with:` prints empty. A checkout step in any
# other layout prints `unparsed checkout layout`. Either way, a policy that expects exact
# records fails closed.
checkout_steps() {
  awk '
    function flush() {
      if (checkout) print "ref=" ref " path=" path " persist-credentials=" persist
      checkout = 0; inputs = 0; ref = ""; path = ""; persist = ""
    }
    /^[[:space:]]*(#|$)/ { next }
    { indent = match($0, /[^ ]/) - 1 }
    /uses:.*actions\/checkout@/ { seen++ }
    indent < 6 || /^      - / { flush() }
    /^      -? *uses: actions\/checkout@/ { checkout = 1; parsed++ }
    checkout && indent == 8 { inputs = ($0 ~ /^        with:[[:space:]]*$/) }
    inputs && /^          ref: / { sub(/^          ref: /, ""); ref = $0 }
    inputs && /^          path: / { sub(/^          path: /, ""); path = $0 }
    inputs && /^          persist-credentials: / { sub(/^          persist-credentials: /, ""); persist = $0 }
    END {
      flush()
      if (seen != parsed) print "unparsed checkout layout"
    }
  ' "$1"
}
