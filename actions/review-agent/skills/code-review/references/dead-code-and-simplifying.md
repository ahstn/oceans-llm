# Dead Code and Simplification

Use this reference for obsolete paths, compatibility code, duplicated policies, or abstractions that add work without a clear benefit.

Look for a behavior-preserving change that removes whole branches, modes, wrappers, or duplicated invariants. Prefer using the layer that already owns the concept. A local direct flow can be clearer than an extraction; a cohesive shared contract can be clearer than repeated policy checks. Compare both against real callers.

Before recommending removal, check whether the path supports migration, rollback, external consumers, reflection, registration, or a documented compatibility contract. An unused-looking export is a lead, not proof. Do not add a replacement abstraction merely to delete a few lines.

Describe the concrete cost and the proposed ownership or state-model change. Identify which branches or duplicate updates disappear and which behavior must remain. Do not demand a broad redesign when a local remedy is sufficient, or prescribe a speculative design without checking its fit.

Review only; do not delete code. Missing intent belongs in a material limitation when it prevents a supported finding. File and function sizes trigger investigation rather than automatic findings.
