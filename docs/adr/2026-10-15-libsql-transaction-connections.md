# ADR: LibSQL Transactions Use Their Own Connections

- Date: 2026-10-15
- Status: Accepted

## Context

`LibsqlStore` holds one `libsql::Connection`, and every store call runs on it. SQLite tracks transactions per connection, not per caller. So when two requests used that connection at the same time, the second `BEGIN` failed with `cannot start a transaction within a transaction`. Statements from any other request also ran inside whichever transaction was open, and were committed or rolled back with it.

The failure showed up as a flaky e2e test. The admin budget update (`PUT /api/v1/admin/spend/budgets`) returned `500` when it collided with background work such as request logging or the agent-analysis queue. The seeded service account's budget then stayed at the test's lowered limit, so later tests on that account got `422 budget_exceeded`. Skills ([Authenticated Agent Skills](2026-10-03-authenticated-agent-skills.md)), model routing, and rate limits already opened their own connections for this reason. The other 23 transactions did not.

## Decision

Every libSQL transaction runs on a connection of its own. `LibsqlStore::begin_transaction` opens a connection from the shared `libsql::Database`, applies the same configuration as the main connection (foreign keys on, a five-second busy timeout), and begins an `IMMEDIATE` transaction. The returned `libsql::Transaction` holds the connection and closes it when the transaction is dropped. Plain single-statement reads and writes stay on the shared connection.

## Implementation

- Store code never calls `.transaction()` on `self.connection`. New transactions use `begin_transaction`, or a dedicated connection as skills and routing already do.
- `IMMEDIATE` takes the write lock at `BEGIN`. A deferred transaction that reads and then writes can get `SQLITE_BUSY` straight away when it tries to upgrade its lock while another connection is writing.
- Local libSQL calls are synchronous, so SQLite's busy handler would park a Tokio worker while it waits for the lock, and a burst of waiting writers could starve the runtime, including the task that holds the lock. `begin_transaction` therefore turns the busy handler off for `BEGIN IMMEDIATE`, retries `SQLITE_BUSY` with an async backoff until the same five-second deadline, and then restores the busy timeout. It also retries `SQLITE_LOCKED`, which is how a shared-cache `:memory:` store reports a competing writer. Once the transaction holds the write lock, only its commit can wait, and only for readers on other connections to finish.
- A statement on the shared connection keeps a read lock until it is finalized, which in libSQL means until both its `Rows` and every `Row` from it are dropped. A transaction on another connection cannot commit while that lock is held. So a function that reads through the shared connection must finish those reads before it commits a transaction. `purge_request_logs_older_than` scopes its reads for this reason.
- `libsql_store::transaction_tests` covers a transaction started while the shared connection is inside one, concurrent transactions, a write that waits for another transaction's lock, and a wait for the lock on a single-threaded runtime.

## Rationale

A connection per transaction is the documented SQLite model for concurrent writers, and the store already relied on it for skills, routing, and rate limits. It gives each transaction the isolation callers assume, with no lock held across requests in Rust. A store-wide async mutex around the shared connection would also have stopped the nested `BEGIN`. It would, however, serialize every read behind any open transaction, and it would still not keep plain statements out of another request's transaction.

## Trade-Offs

- Each transaction opens and configures a connection. Opening a local connection costs little compared with the transaction's writes.
- Plain writes on the shared connection still wait on SQLite's busy handler, which holds a Tokio worker while another connection holds the write lock. A transaction now holds that lock on its own connection, so a burst of plain writes during a transaction can occupy every worker until the holder gets one back to commit, or until a writer times out. Skills, routing, and rate limits already created this contention; this decision makes it more frequent.
- In the default rollback-journal mode, a commit waits for readers on other connections, up to the busy timeout. A long read through the shared connection can therefore delay a commit, and a read still open in the same task can block its own commit until the busy timeout expires.

## Follow-Ups

- Run local libSQL work on a bounded blocking pool, as `run_routing_operation` already does for routing, so no lock wait holds a Tokio worker.

- Consider WAL journal mode for libSQL deployments. Readers would then not block commits, which removes the read-lock hazard above. It also changes the database's on-disk files and backup procedure, so it needs its own decision.
