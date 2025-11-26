# Dashboard monitoring API

- Goal: expose dashboard-ready monitoring APIs with real-time server status visibility.
- Status: in progress (wiring APIs + streaming).

- Sub-agents (responsibilities)
  - API/DTO agent: map dashboard components to REST/SSE/Socket.IO payloads and controller routes.
  - Metrics agent: select maintained crate (e.g., `sysinfo` if suitable), wrap collectors, and ensure
    deterministic testing hooks.
  - Realtime agent: design admin-only subscription channel and events using the existing
    `infra::messenger` (Socket.IO) and consider SSE fallback if needed.
  - Authz/testing agent: enforce admin-only access, add guards, and cover with unit/integration tests.
  - Docs agent: update README/examples and note feature flags/runtime expectations.

- [x] Define dashboard components and data surface (resource summary, service health) and align with
      existing auth/guard model; admin-only access for dashboard subscriptions.
- [x] Choose a maintained metrics crate (`sysinfo`) and validate it fits workspace licensing/runtime
      constraints.
- [x] Design DTOs and routes for dashboard snapshots and streaming updates under
      `/dashboard/monitoring` with admin guard.
- [x] Plan real-time delivery using SSE stream (admin-only) with event names/payload schema for live
      server state; Socket.IO can reuse the same payloads if needed later.
- [x] Implement controller/service wiring, state access, and metric collection pipeline; add the
      router mount in `api::Sumida`.
- [x] Add tests (unit + integration) covering snapshot responses and streaming handshake; mock metrics
      where direct system reads are not deterministic.
- [x] Update documentation/examples to describe new endpoints, feature flags, and runtime expectations.
- Note: dashboard SSE stream interval adjusted to 1s for faster updates.
