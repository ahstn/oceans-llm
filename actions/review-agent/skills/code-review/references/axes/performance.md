# Performance & Scalability Axis

Focus on avoidable cost.

Look for:

- repeated work or redundant fetches
- N+1 database or network patterns
- unbounded loops, scans, or pagination gaps
- extra allocations or large object churn in hot paths
- synchronous blocking, unnecessary re-renders, or missing batching

Prefer concrete impact over vague "this may be slow" language.

## Reachable workload and bounded cost

Tie cost to a concrete path and workload: query count, collection size, payload size, concurrency, cache behavior, or repeated rendering. Distinguish a static complexity argument from a measured benchmark. Check existing bounds and batching before reporting an unbounded path. Micro-optimizations and unsupported “may be slow” claims do not meet the evidence bar.

Question serial awaits only when the work is independent and bounded concurrency preserves ordering, rate limits, resource use, and error handling. Parallel work is not automatically simpler. Check tail latency, backpressure, cancellation, and retained memory as well as throughput.

Prefer a remedy that removes repeated work or uses existing batching over a new cache or scheduler without a demonstrated need. Verify invalidation and ownership before recommending shared cached state.
