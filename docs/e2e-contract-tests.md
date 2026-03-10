# End-to-End Contract Tests

The E2E harness boots three processes:

- A deterministic OpenAI-compatible stub upstream.
- The built admin UI SSR server.
- The real gateway, with `/admin/*` served through the gateway proxy.

Run it locally with:

```bash
mise run e2e-test
```

The harness uses fixed seed credentials so the browser and API assertions stay deterministic:

- Bootstrap admin email: `admin@local`
- Bootstrap admin password: `admin`
- Bootstrap admin replacement password: `s3cur3-passw0rd`
- Seed gateway API key: `gwk_e2e.secret-value`

Scope rule:

- Treat the harness as a contract suite for live gateway-backed flows.
- Today that means admin auth/session/password rotation and `/v1/*` gateway requests.
- Preview-data pages such as API keys, models, and observability can appear as landing assertions, but they should not become standalone business-flow tests until their data is live.

Extension rule:

- Add new browser scenarios only when the page is backed by a real gateway contract.
- Prefer one critical cross-layer flow per new live surface instead of broad UI coverage.

Next recommended additions:

- Password invite completion.
- Team and user management flows once the first harness slice is stable.
