# shellcheck shell=bash
# Sourced by the CI policy checks; uses only awk so it runs on hosts without ripgrep.
#
# Prints one line per actions/checkout step in a workflow: the step's own ref, path and
# persist-credentials inputs. Steps sit at six spaces and their `with:` inputs at ten, as
# in the repository's workflows. An input in any other layout prints empty, so a policy
# that expects a value fails closed.
checkout_steps() {
  awk '
    function flush() {
      if (checkout) print "ref=" ref " path=" path " persist-credentials=" persist
      checkout = 0; ref = ""; path = ""; persist = ""
    }
    /^      - / { flush() }
    /^      -? *uses: actions\/checkout@/ { checkout = 1 }
    /^          ref: / { sub(/^          ref: /, ""); ref = $0 }
    /^          path: / { sub(/^          path: /, ""); path = $0 }
    /^          persist-credentials: / { sub(/^          persist-credentials: /, ""); persist = $0 }
    END { flush() }
  ' "$1"
}
