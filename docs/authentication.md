# Authentication and account security

> **To whoever edits this document.** It states what is true now. It quotes no code, names no file
> or line, and keeps no history; a reason that still matters is written as a present constraint.
> Rewrite the part that changes rather than appending to it.
>
> The rule language and the grant model are [`sz-language.md`](./sz-language.md) and
> [`permissions.md`](./permissions.md). This document covers how a request comes to be someone, and
> the limits on the routes that let an account be taken over.

## 1. Three ways in

A request is someone in one of three ways, and every one of them is re-checked against the account
on every request: the account must exist, be enabled, and not be caught by a live ban, and a
credential established before the account last reset its credentials is refused.

| credential | carried in | lives for |
|---|---|---|
| session cookie | the cookie set by a password sign-in | until it expires, is signed out, or a reset voids it |
| bearer token | `Authorization: Bearer <access token>` | that one request |
| API key | `Authorization: ApiKey <key>` | that one request |

**A header credential is established for its own request and for nothing after it.** A bearer token
and an API key are presented again with every request and checked again with every request — that
re-check is what makes a revoked key, a deleted key or a blacklisted token stop working at once. So
what they establish is never written into the cookie session, and a response to such a request never
sets a session cookie. A client that wants a cookie signs in with a password.

A request that carries both a header credential and a cookie is the header credential's; the cookie
session is left exactly as it was.

## 2. API keys

An **unsecured** key is its owner: it is judged as the owner's user, role and organization, with
everything those hold.

A **secured** key is only itself: it is judged as the key and holds only what was granted to the key.
It reaches none of its owner's grants and none of its owner's platform authority, even when the owner
is an administrator. The administrator routes, impersonation, an account's security fields and
issuing a key in another account's name ask the rule (`platform:root#superuser`) rather than reading
the account's administrator flag, so a secured key cannot borrow its owner's.

Impersonation hands back a cookie session, so it is refused to a request that came in on a header
credential.

A secured key can issue only secured keys, and cannot change its owner's password or email.

## 3. The session cookie

The cookie's `SameSite` attribute is `session.cookie.same_site`: `lax`, `strict` or `none`, read
without regard to case. **Left unset it is `Lax`**, so a page on another site can link to the
service but cannot submit a form or send a background request that carries the cookie — none of the
state-changing routes can be driven from another site on a visitor's behalf. `none` is for a
deployment whose pages are served from another site; such a deployment has to protect its
state-changing routes itself. Any other value is refused at startup.

A password reset (by a verification code or through `/me`) moves the account's reset moment, which
voids every session and token established up to it.

## 4. Changing one's own account

A new password, or an email other than the one on file, is the change that takes an account over:
the password signs in, and the email is where reset codes go. So `PATCH /me` asks for proof beyond the
credential the request carries — `currentPassword`, or a `verificationCode` of kind `account-change`
sent to the account. A wrong current password counts towards the sign-in lockout exactly as a wrong
sign-in does. The cookie session that made a password change is issued anew and stays signed in;
every other session and token of the account is void.

## 5. Verification codes

`POST /user-verifications` sends a six-digit code good for three minutes; `kind` names what it will
be spent on. A code is only ever stored together with the mail that carries it — over `email`, in a
deployment built with mail, to an account with an address on file. Asked for over any other channel,
or where there is no mail, no code is made at all: a code that reaches nobody is one only a guesser
can spend, and its owner, never told, has no reason to change anything.

Six digits are kept out of reach by how few guesses anyone gets:

- **Per code.** One code may be presented `max_failed_attempts` times. The presentation that reaches
  that number spends the code whether it was right or not, and a new one has to be asked for.
- **Per account and per address.** Asking for codes, and presenting them, are each limited to a
  number per window, counted per account and, separately, per address. Past a limit the answer is
  `429`. An account that does not exist is counted just the same, so the answer says nothing about
  which accounts do. One consequence is deliberate: anyone can use up an account's allowance and hold
  its reset back for the rest of the window, which is the price of keeping the code unguessable.

The counts are kept in the process. A restart forgets them, and each instance of a deployment counts
for itself; the defaults are set so that even several instances leave a code out of reach.

| setting | default | meaning |
|---|---|---|
| `security.verification.max_failed_attempts` | `5` | presentations one code allows |
| `security.verification.issue_limit` | `5` | codes asked for per window, per account and per address |
| `security.verification.attempt_limit` | `20` | codes presented per window, per account and per address |
| `security.verification.window` | `"1h"` | the span the two limits are counted over |

## 6. Refresh tokens

A refresh token buys exactly one new pair. Spending it takes it out of the store in one step, so of
any number of requests presenting the same token at once, one gets a pair and the rest are refused.
A token store implemented outside sumida has to provide that step itself.

## 7. Sockets

A socket connection to the `/user` namespace is admitted on the same terms as an HTTP request: an
authorized cookie, an account that is enabled, not banned and not reset since. A connection is
refused when its `Origin` names a page that is neither one of `server.cors.allow_origins` nor the
server's own — CORS does not apply to a WebSocket upgrade, so this is what keeps another site's page
from opening a socket with a visitor's cookie. A handshake with no `Origin` did not come from
another site's page and is judged on its credentials alone. The administrator room is joined by an
account that is an administrator now, and never by a key that carries only its own grants.

## 8. Writes held to the row they reach

The route gate (`service:<namespace>#create` and the rest) says a caller may use an API; these writes
also ask about what they touch:

- **Opening an account.** A caller who is not a platform administrator opens accounts in their own
  organization, never as administrators, never under an id of their choosing, and only in their own
  role or one they may grant (`role:<id>#grant`).
- **Removing an account** removes every grant held by it, by its keys, and on it. The tuple table has
  no foreign key to accounts, so grants left behind would pass to whoever later held the same id.
  Removing an organization does the same for its member set and the grants on it.
- **Inviting** needs `sumida_user:<id>#edit` on the invitee, and inviting a disabled account — which
  the invitation would enable — is a platform administrator's.
- **Bans.** A ban that names an account is written by whoever may edit that account; a ban that names
  only an address needs the ban namespace's operator tier; a ban that names neither is refused for
  everyone, because it would catch every request. Changing a ban asks the same about what it will
  name afterwards, and a ban records who wrote it.
- **Organizations.** Placing an organization under a parent, or taking it out from under one, needs
  `organization:<parent>#edit` on that parent — both the one it leaves and the one it joins. An id is
  chosen by the server unless a platform administrator names it.
- **Replacing a holder's service grants** (the role and API-key forms) needs
  `service:<namespace>#grant` for every namespace whose grants change, including the ones taken away.
  A namespace sent back exactly as it stands asks nothing.
