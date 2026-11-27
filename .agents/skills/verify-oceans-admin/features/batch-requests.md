# Batch requests

Batch Requests lists the gateway batch jobs visible to the signed-in user: every job for platform administrators, and only the user's own jobs for everyone else. Each row shows created time, model and batch ID, provider and endpoint, caller, status, progress, and cost. Users can filter by creation date and status, and platform administrators can also filter by user and service account. A row opens a `Batch responses` sheet with a summary and paginated normalized results. Cancellable batches can be cancelled after confirmation. While any listed batch is active, the list refreshes every five seconds.

## Sub-features

- `batches-list` shows the `Batch list` card, a `Showing N of M` count in `batch-toolbar`, and a desktop table or mobile cards.
- `batches-filter` adds `Status`, `User`, `Service account`, `Created after` and `Created before` chips from the `Filters` menu. Each applies to the URL search as soon as a value is picked.
- `batches-detail` opens `Batch responses` with a summary, a `Response` or `Error` block per `custom_id`, and `Request payload`.
- `batches-cancel` asks `Cancel this batch?` before cancelling a queued, validating, in-progress, or finalizing batch.
- `batches-poll` refreshes the list every 5 s while an active batch is visible, and on `Refresh`.

## How to get to it (user POV)

- Sign in and choose `Batch Requests` under `Observability`.
- Open `/admin/batches` directly, optionally with `?status=completed` or other filter params.
- Choose `View` on a table row, or `View responses` on a mobile card, to open that batch's responses.

## Driving it with control-oceans-admin

Preconditions:

- `control-oceans-admin doctor` passes.
- `mise run dev-stack` seeded two `openai-fast` batches:
  - `Completed`: 2 of 2, from a seeded user key, custom IDs `retention-summary` and `risk-review`.
  - `Queued`: 0 of 3, from `Local CI Runner`.
- You are signed in as `admin@local`. The sidebar entry requires the `request_logs` page. The platform-admin role unlocks all-caller scope and the user and service-account filters.

- **Open.** Choose the `Batch Requests` link. The `Batch requests` heading, `Batch list` card, `Showing 2 of 2`, and `batch-desktop-table` are visible.
- **Filter.** Open `Filters`, choose `Status`, then pick `Completed`. The URL gains `status=completed` and the count reads `Showing 1 of 1`. Choose `Clear` beside the chips to return to 2.
- **Owner filters.** The `User` and `Service account` chips (`batch-filter-user-id`, `batch-filter-service-account-id`) list the seeded users and `Local CI Runner`. Picking `Local CI Runner` leaves only the queued row.
- **Detail.** Choose `View` in the `Completed` row. The `Batch responses` sheet shows `Batch ID`, `Status` `Completed`, `Progress` `2 of 2`, and `Showing 1-2 of 2 responses.`, with two `Succeeded` results. Expand `Request payload` on one of them.
- **Cancel dialog.** Choose `Cancel` in the `Queued` row. When `Cancel this batch?` opens, choose `Keep batch`. The dialog closes and the row still reads `Queued`.
- **API parity.** With the same session, fetch `GET /api/v1/batches?page=1&page_size=30` and `GET /api/v1/batches/{batch_id}/results?page=1&page_size=100`. `total`, `status`, `request_count`, and the result `custom_id` values must match the list and sheet. These responses have no `data` envelope.
- **Proof.** Capture the list, the filtered count, the detail sheet, and the open cancel dialog. Record both batch IDs.

## Gotchas

- Confirming `Cancel batch` can't be undone. The seed only re-creates a batch whose idempotency key is absent, so a cancelled demo batch stays cancelled. Cancel only with operator approval, then restore with `mise run //gateway:reset-local-demo`.
- The queued demo batch keeps the list polling every 5 s. Wait for the expected state, not for network idle.
- Every row has its own `View` and `Cancel` buttons, so scope clicks to the row that contains the model or batch ID.
- Below the `md` breakpoint, `batch-mobile-list` replaces the table and the button reads `View responses`.
- Day chips pick UTC days: `Created after` starts at 00:00 UTC and `Created before` ends at 23:59:59.999 UTC on the day picked. Type any other day as `YYYY-MM-DD`.
