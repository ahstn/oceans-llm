# Design & Architecture Axis

Focus on system fit.

Look for:

- violations of module boundaries or ownership
- dependency direction problems or new coupling across layers
- duplication that should be shared, or premature sharing that should stay local
- new patterns that diverge from the codebase without strong reason
- abstractions that are too broad, too leaky, or too generic for the actual need

Do not rewrite the architecture in your head. Judge the actual change in context.

## Canonical ownership and proportionate restructuring

Be ambitious about a simpler design within the changed behavior. The instruction above rejects speculative redesign, not evidence-backed restructuring. Identify existing extension points and utilities before proposing a new runtime, policy layer, or parallel implementation.

Trace dependency direction, state ownership, and the sites that must change together. Look for feature checks spread through unrelated shared flows, duplicated policy, and implementation details exposed in public contracts. Show a concrete maintenance cost and a feasible ownership or state-model change that removes it.

Prefer fewer concepts over more files. Keep tightly coupled logic together when extraction would force readers through helpers meaningful only in sequence. Share a canonical invariant when independent copies can drift; retain local duplication when the contracts differ. Respect staged migrations, compatibility, and rollback needs.

Review rollout order, in-flight state, old/new schema compatibility, and removed entry points when the diff changes them. These are investigation questions, not a quota for broad concerns. Use a valid changed anchor or the GitHub summary for supported concerns without one.
