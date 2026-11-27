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
is an administrator. Every place that lets a platform administrator past a check asks the rule
(`platform:root#superuser`) rather than reading the account's administrator flag, so a secured key
cannot borrow its owner's: the administrator routes, impersonation, an account's security fields,
issuing a key in another account's name, a post's password and content visibility, the reaction and
viewer lists a board keeps to their author, naming another account as the owner of a billing
request, and reading every account's purchases.

Rows the key's owner owns stay the owner's: what a key writes is written in its owner's name, and a
route that answers about the caller's own rows — their purchases, their personal credit — answers
about the owner's.

Impersonation hands back a cookie session, so it is refused to a request that came in on a header
credential.

A secured key can issue only secured keys, cannot turn a key it may edit into an unsecured one, and
cannot change its owner's password or email. The profile a secured key's request carries never holds
its owner's administrator flag, so a reader that looks at that flag instead of asking the rule is not
fooled by the key either.

## 3. The session cookie

The cookie's `SameSite` attribute is `session.cookie.same_site`: `lax`, `strict` or `none`, read
without regard to case. **Left unset it is `Lax`**, so a page on another site can link to the
service but cannot submit a form or send a background request that carries the cookie — none of the
state-changing routes can be driven from another site on a visitor's behalf. `none` is for a
deployment whose pages are served from another site; such a deployment has to protect its
state-changing routes itself. Any other value is refused at startup.

A password reset (by a verification code or through `/me`) moves the account's reset moment, which
voids every session and token established up to it. So does accepting an invitation, and so does
another account changing this one's password, administrator flag, role, organization or enabled state:
a session carries the role, organization and flag it was established with, so one left standing would
go on acting with what was just taken away.

### The sign-in lockout

A wrong password is written to the access log, and once the unbroken run of wrong passwords reaches
`security.ban.max_failed_attempts` a ban is written. What is counted is what is banned: with
`security.ban.target` `ip`, the run is every wrong password from the request's address, whichever
account each named, and the address is banned; with `account`, the run is that account's wrong
passwords from anywhere, and the account is banned. Counting one and banning the other would let a
guesser spread guesses so that no run ever reached the limit. A run ends at a sign-in that succeeds
from the same address, or of the same account; an address's run also ends at the last ban written on
that address, so once a ban is lifted the failures that earned it are not counted again. A gap wider
than `security.ban.failure_reset_after` ends a run as well.

The address is the one the deployment's `session.ip_source` names. Behind a proxy it must be the
header that proxy sets, and the server must not be reachable around the proxy: whoever reaches it
directly writes that header themselves, and with it both escapes the count and gets any address they
name banned.

### Requests from another site's page

`SameSite` keeps the cookie off most requests another site's page sends, but not all of them: `none`
sends it everywhere, and a sibling subdomain counts as the same site under `lax` and `strict`. So the
guard also asks where a request that would change something came from. A request authenticated by
the cookie, with any method but `GET`, `HEAD`, `OPTIONS` or `TRACE`, is refused (`403`, `E1019`) when
the page it names is neither the server's own nor one of `server.cors.allow_origins`. The page is the
request's `Origin`, or, when the browser leaves that out, the origin of its `Referer`; `null` names no
page this server answers. The server's own origin is the one the request was addressed to — its
`Host` — so a deployment behind a proxy that rewrites `Host` lists its public origin in
`server.cors.allow_origins`. A request that names no page at all is left to its cookie, since a
browser names the page on every request another site's page makes.

A request that carries a header credential is not asked: another site's page cannot add one to a
visitor's request, so the header already shows the request was not forged.

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
refused when the page it names is neither one of `server.cors.allow_origins` nor the server's own,
judged exactly as a state-changing request is (§3) — CORS does not apply to a WebSocket upgrade, so
this is what keeps another site's page from opening a socket with a visitor's cookie. A handshake
that names no page did not come from another site's page and is judged on its credentials alone. The administrator room is joined by an
account that is an administrator now, and never by a key that carries only its own grants.

## 8. Writes held to the row they reach

The route gate (`service:<namespace>#create` and the rest) says a caller may use an API; these writes
also ask about what they touch:

- **Opening an account.** A caller who is not a platform administrator opens accounts in their own
  organization, never as administrators, never under an id of their choosing, and only in their own
  role or one they may grant (`role:<id>#grant`).
- **Administrators are a platform administrator's.** Whoever may edit an account can set its
  password and email, ban it or remove it, each of which takes it over or shuts it out. So editing,
  inviting, banning or removing an account that is an administrator is refused to anyone who is not a
  platform administrator, whatever they hold over the account's organization or the user namespace.
- **Removing an account** removes every grant held by it, by its keys, and on it. The tuple table has
  no foreign key to accounts, so grants left behind would pass to whoever later held the same id.
  Removing an organization does the same for its member set and the grants on it.
- **Inviting** needs `sumida_user:<id>#edit` on the invitee, and inviting a disabled account — which
  the invitation would enable — is a platform administrator's. An invitation's token sets the
  account's password, so a list of invitations shows the token to a platform administrator alone.
- **Roles.** A role's id is chosen by the server unless a platform administrator names it, and
  removing a role removes every grant held by its members as members and every grant on it, so a
  role opened later under the same id comes with none of them.
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
