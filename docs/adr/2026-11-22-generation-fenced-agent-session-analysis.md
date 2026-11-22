# Generation-fenced agent-session analysis

- Status: accepted
- Date: 2026-11-22
- Follows: [Atomic agent-session input](./2026-11-22-atomic-agent-session-input.md)

## Context

A queue item for every request caused workers to rebuild the same growing session repeatedly. Loading observation sets required two queries per set. Session listing also hydrated complete traces, even for cohort selection and idle finalisation.

Timestamp watermarks cannot identify an input revision. A late request can change facts without advancing the maximum timestamp. An older worker must not publish a mixed snapshot or clear work requested while it held a lease.

## Decision

### One durable job per session

Keep one queue row per session, with the latest requested analysis policy. An incoming request ID remains its deduplication token; it does not replace the durable row's identity. Repeating the current token, policy and input generation does nothing. A new token, policy or generation advances the request revision.

Track three separate identities:

- **Input generation** identifies committed request, observation and finalisation changes. Event time still controls request ordering and display.
- **Request revision** includes operator and policy requests that change no input facts.
- **Lease epoch** increments on every claim, including recovery by the same worker name. Renewal, completion, failure and publication carry that fencing identity.

A claim snapshots the requested generation and revision. New work leaves an active lease intact but dirty. Completion or failure returns dirty work to pending instead of clearing it. A new revision receives a fresh retry budget; retries of an unchanged revision retain their attempt limit. A worker cannot renew or complete a reclaimed lease.

Production ingestion schedules its analysis request inside the input transaction. Finalisation accepts a prepared analysis request and commits it with the lifecycle transition, including when no prior job exists. Observation-only inputs, or low-level finalisation without a supplied policy, wake an existing requested job. They do not invent a policy when analysis has not been requested. Explicit recomputation can create that job later. Each CLI invocation has a fresh request token, so an operator can recompute unchanged input after a failure or an external evidence change.

### Guard publication, not just scheduling

A report records the input generation captured before its fact reads. Publication takes the session writer lock, checks that generation, then checks the active queue lease and request revision. It inserts the report and supersedes older reports in that transaction. PostgreSQL uses the same foreign-key-compatible `FOR NO KEY UPDATE` session lock as ingestion, followed by the queue lock; queue claiming never takes a session writer lock. libSQL uses its write transaction.

A generation or lease mismatch discards superseded work without publication. The dirty queue revision remains available for another run. Report identity and the database uniqueness constraint include generation, so equal timestamps and an unchanged latest observation-set ID do not collide after late input. Reads exclude reports from earlier generations immediately; there is no separate, racy blanket stale update after ingestion or publication.

Generation covers correlated session inputs. Pricing, cohort and direct-MCP evidence retain their version and snapshot provenance; an explicit recomputation can refresh them without changing the input generation. Multiple fact reads need not hold a long read transaction: a concurrent correlated-input commit changes generation and prevents publication of the mixed candidate.

### Read summaries and bulk facts

Load observation-set metadata first, then fetch observations by those selected IDs. PostgreSQL binds an array; libSQL uses batches of 500 IDs. Preserve set ordering by source watermark, creation time and ID, and observation ordering by event time and ID. This removes per-set hydration while retaining the bounded oversize sentinel.

Session listing now returns session metadata, scalar request count, compact source labels and a bulk-loaded current report. It does not load request links or observation JSON. Cohort selection and the admin list use that contract. Idle finalisation uses a separate session-only query over the existing lifecycle/watermark index, avoiding report hydration and page counts entirely. Detail reads still load a complete bounded trace.

### Migration

V86 requires a generation-aware reader. It preserves report history and supersession links, marks timestamp-only reports stale, and includes generation in report identity and the latest-report index. It collapses disposable queue history to the latest requested policy per session and returns that job to pending with a fresh lease. Do not run older writers against the upgraded schema.

## Consequences

A busy session may require another run before it becomes quiet enough to publish a current snapshot. This favours correct provenance over showing an apparently current report built from mixed input. Coalescing discards redundant queued rebuilds, not request facts or report history.

Only the latest command token is retained, rather than an unbounded history of recomputation requests. Replaying an older operator command can request another rebuild; report identity still prevents duplicate publication of identical provenance. Replayed ingestion facts do not schedule work because their input transaction makes no change.

The session remains the ordering and capacity boundary. Lock order is session, queue, then report publication; ingestion visits its request and observation children before scheduling. Pure analysis stays outside writer transactions. Summary queries still load report data when consumers need metrics, but never hydrate their traces. The explicit store methods and SQL remain backend-specific rather than introducing a query-building abstraction.

Backend contracts cover late input with an unchanged watermark, manual policy changes during a lease, dirty success and failure, expired same-owner leases, publication fencing, scheduling rollback, initial finalisation scheduling, migration history preservation, bulk read/write boundaries and strict idle selection. They run through the existing libSQL and PostgreSQL contract harnesses.
