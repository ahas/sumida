# Membership / Billing — Operations Guide

The membership domain adds subscription + credit plans, a FIFO credit ledger with
reservations, a hybrid entitlement gate, coupons/tax, a PG abstraction (PortOne
V2 first-class), and a background maintenance scheduler.

- Entities: `entity/src/membership/*`, migrations `entity/src/migration/membership*.rs`
- Services: `api/src/service/membership/*`
- Controllers nest at: `/membership-plans`, `/subscriptions`, `/credits`,
  `/entitlements`, `/membership-purchases`, `/payments`, `/coupons`

This guide covers what an operator must configure before the feature works.

## 1. Required access-policy seeds

`Sumida::new().router()` nests the routes, but most actions are gated. Seed these
rows in `access_policies` (the `user:admin` wildcard from the test harness bypasses
all of them; production needs explicit policies).

| Namespace | Object | Who needs it | Example action(s) |
|---|---|---|---|
| `membership_plan` | `service` | plan-catalog managers (non-admin) | `read`, `create`, `update`, `delete` |
| `org:subscription` | `<org_id>` (text) | organization subscription managers | `read`, `manage` |
| `org:credit` | `<org_id>` (text) | organization credit managers | `use`, `manage` |

Plan management (`/membership-plans` write) is gated by `service_ap!(state,
"membership_plan", …)`; system admins (`profile.is_admin`) and holders of a
matching `membership_plan` policy pass. **Coupon** management (`/coupons`) is
**system-admin only** (no AP seed possible — set `users.is_admin`).

Personal subscriptions/credits need no seed (owner = the session user). Org
actions require an `org:subscription` / `org:credit` allow policy whose `object`
is the org id; the org-ancestors subquery makes a parent-org policy apply to all
descendant orgs automatically.

```sql
-- Let role 7 manage the plan catalog:
INSERT INTO access_policies (namespace, object, action, effect, "user")
VALUES ('membership_plan', 'service', ARRAY['read','create','update','delete'], 'allow', 'role:7');

-- Let role 9 manage organization 42's subscriptions (and its sub-orgs):
INSERT INTO access_policies (namespace, object, action, effect, "user")
VALUES ('org:subscription', '42', ARRAY['read','manage'], 'allow', 'role:9');

-- …and its credit wallet:
INSERT INTO access_policies (namespace, object, action, effect, "user")
VALUES ('org:credit', '42', ARRAY['use','manage'], 'allow', 'role:9');
```

The `user` column must use the `ApUser` string form: `user:<id>`, `role:<id>`,
`org:<id>`, `api:<key>`, or `user:admin`.

## 2. Settings (`[membership]`)

See `settings.example.toml` for the full annotated block. Keys (kebab-case):

| Key | Default | Meaning |
|---|---|---|
| `default-currency` | `KRW` | currency when a plan/wallet omits one |
| `default-gate-order` | `subscription-first` | entitlement order: `subscription-first` \| `credit-first` |
| `credit-default-expiry-days` | none | default expiry for granted credit lots |
| `expiring-soon-notify-days` | `7` | lead time for the "expiring soon" notice |
| `credit-low-balance-threshold` | none | low-balance alert threshold (disabled if unset) |
| `grace-days` | `0` | entitlement grace after a failed renewal (0 = none) |
| `max-dunning-attempts` | `0` | renewal-charge retries during grace (0 = none) |
| `tax-percent-bp` | `0` | checkout sales tax in basis points (1000 = 10%) |
| `[membership.payment] provider` | `none` | `none` \| `portone` \| `mock` |
| `[membership.payment] api-secret` / `store-id` / `channel-key` / `webhook-secret` | — | PortOne V2 credentials |

## 3. Background scheduler

All expiry/renewal/notification logic is *lazy* unless the scheduler runs it.
Start it once after building your state:

```rust
sumida::api::service::membership::maintenance::spawn(
    state.clone(),
    std::time::Duration::from_secs(3600), // hourly is typical
);
```

`run_once` performs, in order: auto-renew due subscriptions → dunning retries →
expire ended/grace-closed subscriptions → release expired reservations → sweep
expired credit lots → "expiring soon" notices → "low balance" alerts. Each job is
idempotent and independently fault-isolated.

**Deployment caveat — single runner.** The scheduler holds no distributed lock,
so running it in N application instances runs every job N times per tick. Run it
in exactly one instance (a dedicated worker, or a leader-elected node), or wrap
`run_once` behind a Postgres advisory lock / external scheduler if you must spawn
it everywhere. The per-row transactions make double-runs *safe* (idempotent) but
wasteful and noisier in logs.

## 4. Mail templates

The `membership` migration seeds four `mail_forms` with English placeholder text:
`subscription-created-receipt`, `subscription-expiring-soon`,
`credit-purchased-receipt`, `credit-low-balance-alert`. Replace the
sender/subject/body for production (notices are best-effort and only sent to
**personal** owners with an email; organization owners have no mail channel).

## 5. Payment flow (PG)

`POST /payments/checkout` → pending purchase (coupon + tax applied, total
recorded with breakdown) → client pays at the PG → `POST /payments/confirm` (or
the `Transaction.Paid` webhook) → server-side `verify` (PAID + amount match) →
subscription activated / `plan.credit_amount` credits granted. Auto-renew,
dunning, and a positive plan-change proration charge the stored `billing_key` via
`charge_recurring` (a declined proration charge aborts the change). Refund
reverses the entitlement and cancels at the PG. Webhooks are Standard-Webhooks
signature-verified and idempotent (deduped by `webhook-id`).

A free trial (`plan.trial_days`) is granted at most once per (owner, plan); a
later subscribe to the same plan starts a normal paid period.
