# ADR: Durable Provider Batch Processing

- Date: 2026-08-17
- Status: Accepted

## Context

Provider batch APIs can run for hours. The gateway must return an acceptance response before the provider result exists, survive process restarts, prevent two workers from updating the same job, and represent a submission whose upstream outcome is uncertain.

Provider APIs do not use one common contract. OpenAI uses uploaded JSONL files, OpenRouter uses a JSON batch resource, and Vertex uses BigQuery tables and batch prediction jobs. Network failures can occur before or after an upstream job is created.

## Decision

### Durable jobs and normalized results

The gateway stores each accepted batch, its input items, its selected route, and the provider request context before provider submission. A background worker submits and polls the provider job. Terminal provider results are normalized into gateway batch items before clients retrieve them.

### Fenced renewable leases

Workers claim due jobs with a unique lease owner and an expiry time. All worker writes include the lease owner as a fence. The worker renews the lease during long provider operations. If renewal fails, the in-flight provider future is allowed to finish so a sent submission is not cancelled at an unknown point. The fenced store write then determines whether that worker still owns the job.

Definite retryable submission failures return the job to `queued`. Retryable poll and cancellation failures preserve the current state and set a later poll time. Permanent provider failures move the job to `failed`.

### Submission uncertainty

Providers classify create failures as `NotSubmitted` or `SubmissionUnknown`. A provider adapter can reconcile an uncertain create by searching all provider list pages for the gateway batch identifier. If reconciliation cannot prove that a job exists, the gateway stores `submission_unknown`. It does not submit the job again automatically because that could create duplicate provider work and cost.

### Access and accounting

Batch list, detail, result, and cancellation operations use the same ownership scope as request logs. Platform admins can access all batches. Other signed-in users, including team admins, can access only batches owned by their user. API-key callers can access only batches created by that key.

The worker prices terminal normalized results and writes one idempotent usage ledger event for the batch. The event contains the final provider usage returned by the terminal inspection. A priced status without a cost is invalid.

## Consequences

Benefits:

- process restarts do not lose accepted batch work
- lease fencing limits duplicate local processing
- uncertain provider submissions do not retry automatically
- clients use one result shape across providers
- batch records follow the request-log visibility model

Trade-offs:

- provider reconciliation depends on each provider's list and metadata features
- `submission_unknown` can require manual provider-side investigation
- input items and normalized results require durable storage
- polling adds provider API traffic and delays state visibility by the poll interval

## Follow-up work

- Add operator tools to reconcile or close `submission_unknown` jobs.
- Add metrics for lease loss, reconciliation pages, polling latency, and terminal provider errors.
- Add retention settings for batch inputs and results.
- Verify provider billing and cancellation behavior with live acceptance tests.
