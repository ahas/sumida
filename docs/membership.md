# Membership / Billing — Operations Guide

The membership domain adds subscription + credit plans, a FIFO credit ledger with
reservations, a hybrid entitlement gate, coupons/tax, a PG abstraction (PortOne
V2 first-class), and a background maintenance scheduler.

- Entities: `entity/src/membership/*`, schema in the single `entity/src/migration/membership.rs`
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
| `default-currency` | `KRW` | currency when a plan omits one (credits are unitless) |
| `default-gate-order` | `subscription-first` | entitlement order: `subscription-first` \| `credit-first` |
| `credit-default-expiry-days` | none | default expiry for granted credit lots |
| `expiring-soon-notify-days` | `7` | lead time for the "expiring soon" notice |
| `credit-low-balance-threshold` | none | low-balance alert threshold (disabled if unset) |
| `grace-days` | `0` | entitlement grace after a failed renewal (0 = none) |
| `max-dunning-attempts` | `0` | renewal-charge retries during grace (0 = none) |
| `tax-percent-bp` | `0` | checkout sales tax in basis points (1000 = 10%) |
| `pending-purchase-ttl-hours` | `0` | expire unconfirmed pending purchases (reversing their coupons) after N hours; 0 = off |
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

**Opt-in, not automatic.** `Sumida::new().router()` does NOT start the scheduler;
the downstream app must call `spawn(...)` once (see `examples/basic`). Omit the
call and the maintenance jobs never run.

**Multi-instance safe.** `run_once` is guarded by a Postgres *advisory lock*, so
running the scheduler in N application instances still runs each tick exactly
once — the instance that wins the lock runs the pass, the rest skip immediately.
(The individual job functions are public and lock-free for one-off CLIs.)

## 4. Mail templates

The `membership` migration seeds four `mail_forms` with English placeholder text:
`subscription-created-receipt`, `subscription-expiring-soon`,
`credit-purchased-receipt`, `credit-low-balance-alert`. Replace the
sender/subject/body for production. Notices are best-effort and go to the
**personal** owner's email, or — for organization owners — to the org's
**managers** (everyone holding an `org:subscription` / `org:credit` manage policy
on that org, resolved through their `user:` / `role:` / `org:` identities).

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

**Billing keys** (정기결제 keys) are issued client-side via the PG SDK (card data
never touches the server) and injected via `POST /subscriptions`
(`billingKey` + `autoRenew`) or `POST /subscriptions/{id}/auto-renew`. On
injection the key is verified with the PG (`PaymentProvider::verify_billing_key`)
so an invalid/inactive key is rejected up front rather than failing at the first
auto-renew. No server-side issuance webhook is required.

## 6. Capabilities & limitations

What this domain can and cannot do today.

### ✅ Possible
- **Catalog**: subscription & credit plans (personal/org scope), free trials,
  per-action quotas / credit costs, soft-delete (retire) plans.
- **Subscriptions**: subscribe (free/manual or PG-paid), cancel (now/period-end),
  manual renew, **auto-renew** (billing key), **grace + dunning** retries,
  **reactivation**, **mid-period plan change with proration** (upgrade charges the
  billing key; decline aborts).
- **Trials**: free first period, **once per (owner, plan)** (re-subscribe is paid).
- **Credits**: grant (with expiry, FIFO lots), consume (idempotent), **reserve /
  capture / release** (authorize-capture), refund, balance/spendable, ledger +
  reservation lists, consistency **audit**.
- **Entitlement**: hybrid subscription/credit gate with per-period **quota**;
  route-layer (`entitlement_guard!`) or in-handler (`require_action`).
- **Payments (PortOne V2)**: checkout → confirm/webhook → verify → activate;
  refund; recurring billing-key charge (auto-renew, dunning, proration); webhook
  signature verification + idempotency.
- **Coupons & tax**: percent / fixed-amount coupons (currency-matched), global +
  Each coupon is exactly one reward — a **discount** (`percent_off`, currency-free),
  **credits** (`grant_credits`, redeem → wallet), or a **free plan** (`free_plan_code`,
  redeem → a free subscription period). Discount coupons apply at checkout; reward
  coupons are claimed self-service via `POST /coupons/redeem`. Coupons support
  global + **per-owner** caps, expiry, grant-gating (`requires_grant`),
  activate/retire/delete; **plan-scoping** (valid only for listed plans);
  **currency-scoping** (`currency` = a regional promo valid only for that currency);
  **stacking** (combine `stackable` discount coupons in one checkout). There is no
  fixed-money discount — money lives only on plans/purchases, not coupons. A
  **failed or abandoned checkout reverses** its coupons (the consumed grant/
  redemption is returned) — on a declined `confirm`, and via the scheduler for
  stale pending purchases (`pending-purchase-ttl-hours`).
- **Coupon distribution**: issue a coupon to specific owners (`POST
  /coupons/{id}/grant`, idempotent), **recall** it (`DELETE /coupons/{id}/grant`).
  `requires_grant` makes a coupon redeemable **only** by granted owners and each
  grant is **single-use** (one grant = one redemption); re-issuing a spent grant
  requires a revoke first (no silent re-grant). Mass-issue via `POST
  /coupons/{id}/grant-bulk`. Recipients see theirs via `GET /coupons/mine`; admins
  audit issued/redeemed/revoked via `GET /coupons/{id}/grants`.
- **Organizations**: org-admin (ReBAC) management of org subscriptions/credits with
  automatic parent→child hierarchy (single-row *and* lists); org notices reach the
  org's managers.
- **Scheduler**: background expiry/renewal/dunning/sweep/notices, **multi-instance
  safe** (advisory lock), idempotent.

### ❌ Not (yet) possible
- **Billing-key *issuance* server-side** — keys are issued via the client SDK; the
  server only stores & charges them. A subscription without a stored billing key
  cannot auto-renew (it expires).
- **Downgrade refund / negative-proration payout** — recorded as a breakdown only;
  actual money-out is left to the operator/PG.
- **Per-currency credit wallets / FX** — credits are a unitless internal balance
  (one wallet per owner); money currency lives only on plans/purchases and a
  coupon's discount scope. Multi-currency *plan pricing* (a price book) is the
  open item if regional pricing is ever needed.
- **Usage-based / overage billing** — quotas *block*; they do not bill the excess.
- **Reserve/capture tied to an external PG authorize-capture** — credit holds are
  internal-ledger only.
- **Reward-coupon owner scope** — `POST /coupons/redeem` grants to the calling
  user (personal). Redeeming on behalf of an organization is not exposed yet.
- **Tax tables** — a single flat basis-point rate (no per-region/exemptions/tax id).

See `TASKS.md` ("멤버십 후속") for the deferred items and rationale.
