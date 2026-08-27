//! The document fragment this crate serves for the routes it mounts.
//!
//! What is described here is a set of operations and the components they need — never a whole
//! document. A document has an identity: a title, a version, the servers it is reachable at. That
//! identity belongs to whatever product mounts these routes, so a consumer merges this fragment
//! into its own document and its own `info` is the one that survives.
//!
//! An operation is described **beside** its handler, which means it compiles whether or not it says
//! anything true about what that handler does. Four things narrow the gap: the router and the
//! annotation read the same path constants, the set of routes and the set of descriptions are held
//! against each other by a check, requiredness is put to the real reader, and every shape written by
//! hand here is compared against the value the server actually sends.

use utoipa::openapi::path::{HttpMethod, OperationBuilder, PathItem};
use utoipa::openapi::security::{ApiKey, ApiKeyValue, HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::openapi::{ContentBuilder, OpenApi as OpenApiDocument, Ref, Response, ResponseBuilder};
use utoipa::{IntoResponses, Modify, OpenApi, ToSchema};

use crate::controller::{
  access_log,
  api_key,
  ban,
  organization,
  relation_tuple,
  role,
  user,
  user_invitation,
};
#[cfg(feature = "board")]
use crate::controller::bps;
#[cfg(feature = "file")]
use crate::controller::fs;
#[cfg(feature = "mail-form")]
use crate::controller::mail_form;
#[cfg(feature = "membership")]
use crate::controller::membership;
use crate::layer::session::DEFAULT_COOKIE_NAME;
use crate::util::filter::DESCRIPTOR_PATH;

/// What a reader of the merged document has to know about the part of it that came from here.
///
/// The `info` this sits in is dropped when the fragment is merged, so it is repeated on nothing;
/// what a consumer keeps is the operations, the components and the security schemes below.
///
/// The sentence about instants is here rather than on each field because it holds for all of them
/// at once. `date-time` is RFC 3339, and RFC 3339 admits both a trailing `Z` and a numeric offset,
/// so a field's own entry cannot tell a reader which of the two it will meet. This surface writes
/// one of them, everywhere, and a reader may hold it to that.
const DESCRIPTION: &str = "\
The routes the sumida base mounts. A consumer merges this fragment into its own document, so the \
title, the version and the servers a reader sees are that product's, not this one's.\n\n\
Every `date-time` written by these routes is UTC, ending in `Z` — `2026-01-01T00:00:01Z`. On the \
way in any RFC 3339 offset is accepted and names the instant it names.";

/// The name a caller uses to prove who they are, and the three ways to present it.
///
/// Every guarded route accepts any one of the three. Which one a request used changes nothing about
/// what it may then do, except that a secured API key carries only the grants given to the key
/// itself rather than everything its owner may do.
mod scheme {
  pub const SESSION: &str = "session";
  pub const BEARER: &str = "bearer";
  pub const API_KEY: &str = "apiKey";
}

/// Tags the controllers name from their own annotations, so a controller and this assembly point
/// cannot spell the same group two ways.
pub(crate) const ACCESS_LOG_TAG: &str = "access log";
pub(crate) const API_KEY_TAG: &str = "api key";
pub(crate) const AUTHZ_TAG: &str = "authz";
pub(crate) const RELATION_TUPLE_TAG: &str = "relation tuple";
pub(crate) const MONITORING_TAG: &str = "monitoring";
pub(crate) const OVERVIEW_TAG: &str = "dashboard";
pub(crate) const CAPABILITY_TAG: &str = "capability";
#[cfg(feature = "mail-form")]
pub(crate) const MAIL_FORM_TAG: &str = "mail form";
#[cfg(feature = "file")]
pub(crate) const FILE_TAG: &str = "file";
#[cfg(feature = "board")]
pub(crate) const BOARD_TAG: &str = "board";
#[cfg(feature = "board")]
pub(crate) const BOARD_GROUP_TAG: &str = "board group";
#[cfg(feature = "board")]
pub(crate) const BOARD_TOPIC_TAG: &str = "board topic";
#[cfg(feature = "board")]
pub(crate) const POST_TAG: &str = "board post";
#[cfg(feature = "board")]
pub(crate) const REACTION_TAG: &str = "board post reaction";
#[cfg(feature = "board")]
pub(crate) const REPORT_TAG: &str = "board post report";
#[cfg(feature = "board")]
pub(crate) const BOARD_FILE_TAG: &str = "board file";
#[cfg(feature = "membership")]
pub(crate) const PLAN_TAG: &str = "membership plan";
#[cfg(feature = "membership")]
pub(crate) const SUBSCRIPTION_TAG: &str = "subscription";
#[cfg(feature = "membership")]
pub(crate) const CREDIT_TAG: &str = "credit";
#[cfg(feature = "membership")]
pub(crate) const ENTITLEMENT_TAG: &str = "entitlement";
#[cfg(feature = "membership")]
pub(crate) const PURCHASE_TAG: &str = "membership purchase";
#[cfg(feature = "membership")]
pub(crate) const PAYMENT_TAG: &str = "payment";
#[cfg(feature = "membership")]
pub(crate) const COUPON_TAG: &str = "coupon";
pub(crate) const AUTH_TAG: &str = "auth";
pub(crate) const ME_TAG: &str = "me";

const USER_TAG: &str = "user";
const ORGANIZATION_TAG: &str = "organization";
const ROLE_TAG: &str = "role";
const BAN_TAG: &str = "ban";
const USER_INVITATION_TAG: &str = "user invitation";
const USER_VERIFICATION_TAG: &str = "user verification";

#[derive(OpenApi)]
#[openapi(
  info(title = "sumida routes", description = DESCRIPTION),
  paths(
    crate::controller::user::list,
    crate::controller::user::get,
    crate::controller::user::create,
    crate::controller::user::update,
    crate::controller::user::delete,
    crate::controller::user::invite,
    crate::controller::user::ban,
    crate::controller::user::lift_ban,
    crate::controller::user::impersonate,
    crate::controller::user::activate,
    crate::controller::user::change_password,
    crate::controller::organization::list,
    crate::controller::organization::get,
    crate::controller::organization::create,
    crate::controller::organization::update,
    crate::controller::organization::delete,
    crate::controller::role::list,
    crate::controller::role::get,
    crate::controller::role::create,
    crate::controller::role::update,
    crate::controller::role::delete,
    crate::controller::ban::list,
    crate::controller::ban::get,
    crate::controller::ban::create,
    crate::controller::ban::update,
    crate::controller::ban::delete,
    crate::controller::user_invitation::list,
    crate::controller::user_invitation::get,
    crate::controller::user_invitation::create,
    crate::controller::user_invitation::update,
    crate::controller::user_invitation::delete,
    crate::controller::user_verification::send,
    crate::controller::access_log::list,
    crate::controller::access_log::get,
    crate::controller::access_log::delete,
    crate::controller::api_key::list,
    crate::controller::api_key::get,
    crate::controller::api_key::create,
    crate::controller::api_key::update,
    crate::controller::api_key::delete,
    crate::controller::authz::schema,
    crate::controller::relation_tuple::list,
    crate::controller::relation_tuple::get,
    crate::controller::relation_tuple::create,
    crate::controller::relation_tuple::delete,
    crate::controller::dashboard::snapshot,
    crate::controller::dashboard::snapshot_at_path,
    crate::controller::dashboard::components,
    crate::controller::dashboard::system_info,
    crate::controller::dashboard::stream,
    crate::controller::dashboard_overview::auth_activity,
    crate::controller::dashboard_overview::account_activity,
    crate::controller::dashboard_overview::storage_usage,
    crate::controller::auth::session::login,
    crate::controller::auth::session::logout,
    crate::controller::auth::token::login,
    crate::controller::auth::token::refresh,
    crate::controller::auth::token::logout,
    crate::controller::me::get_me,
    crate::controller::me::update_me,
    crate::controller::me::get_prefs,
    crate::controller::me::update_prefs,
    crate::controller::me::get_access_policies,
    crate::controller::me::get_permissions,
  ),
  // Neither of these is the response of any one operation, so nothing collects them on its own way
  // through: one is written by the error layer as a value rather than as a type, the other stands
  // in for every body that carries nothing.
  components(schemas(ErrorBody, EmptyObject)),
  modifiers(&Credentials),
  tags(
    (name = USER_TAG, description = "Accounts — who exists, what they may be, and how one is put to use"),
    (name = ORGANIZATION_TAG, description = "The tree accounts hang from"),
    (name = ROLE_TAG, description = "Named bundles of grants an account is given by holding one"),
    (name = BAN_TAG, description = "Accounts and addresses shut out, for a while or for good"),
    (name = USER_INVITATION_TAG, description = "The token that lets someone who cannot sign in yet set their own password"),
    (name = USER_VERIFICATION_TAG, description = "The short-lived code that proves a person is reachable where they claim to be"),
    (name = ACCESS_LOG_TAG, description = "Who tried to sign in, by what means, and how it went"),
    (name = API_KEY_TAG, description = "Long-lived credentials issued against an account"),
    (name = AUTHZ_TAG, description = "The vocabulary a grant is written in"),
    (name = RELATION_TUPLE_TAG, description = "The record of who holds what — the grants the evaluator reads"),
    (name = MONITORING_TAG, description = "What the host this deployment runs on is doing right now"),
    (name = OVERVIEW_TAG, description = "Installation-wide counts for the operator's dashboard"),
    (name = AUTH_TAG, description = "The ways in — signing in, signing out, and the tokens that stand in for a sign-in"),
    (name = ME_TAG, description = "The account behind the request, as it sees itself"),
  ),
)]
struct Fragment;

// A build that leaves a feature off does not serve that feature's routes, so the fragment does not
// describe them either. Each optional group is a document of its own, written under the same `cfg`
// the router mounts it under and merged in below — a document that named routes this binary does
// not serve would be as wrong as one that left served routes out.

/// The mail form routes, mounted only where `mail-form` is on.
#[cfg(feature = "mail-form")]
#[derive(OpenApi)]
#[openapi(
  paths(
    crate::controller::mail_form::list,
    crate::controller::mail_form::get,
    crate::controller::mail_form::create,
    crate::controller::mail_form::update,
    crate::controller::mail_form::delete,
    crate::controller::mail_form::upload_files,
  ),
  tags((name = MAIL_FORM_TAG, description = "The templates outgoing mail is rendered from")),
)]
struct MailFormRoutes;

/// The file routes, mounted only where `file` is on.
#[cfg(feature = "file")]
#[derive(OpenApi)]
#[openapi(
  paths(
    crate::controller::fs::file::list,
    crate::controller::fs::file::get,
    crate::controller::fs::file::stats,
    crate::controller::fs::file::download,
    crate::controller::fs::file::inline,
    crate::controller::fs::file::upload,
    crate::controller::fs::file::delete,
  ),
  tags((name = FILE_TAG, description = "Uploads — their records, their bytes, and what they add up to")),
)]
struct FileRoutes;

/// The board routes, mounted only where `board` is on.
#[cfg(feature = "board")]
#[derive(OpenApi)]
#[openapi(
  paths(
    crate::controller::bps::board::list,
    crate::controller::bps::board::get,
    crate::controller::bps::board::posts,
    crate::controller::bps::board::create,
    crate::controller::bps::board::update,
    crate::controller::bps::board::delete,
    crate::controller::bps::board_group::list,
    crate::controller::bps::board_group::get,
    crate::controller::bps::board_group::create,
    crate::controller::bps::board_group::update,
    crate::controller::bps::board_group::delete,
    crate::controller::bps::board_topic::list,
    crate::controller::bps::board_topic::get,
    crate::controller::bps::board_topic::create,
    crate::controller::bps::board_topic::update,
    crate::controller::bps::board_topic::delete,
    crate::controller::bps::board_post::list,
    crate::controller::bps::board_post::get,
    crate::controller::bps::board_post::view,
    crate::controller::bps::board_post::viewers,
    crate::controller::bps::board_post::create,
    crate::controller::bps::board_post::update,
    crate::controller::bps::board_post::delete,
    crate::controller::bps::board_post_reaction::list,
    crate::controller::bps::board_post_reaction::get,
    crate::controller::bps::board_post_reaction::create,
    crate::controller::bps::board_post_reaction::update,
    crate::controller::bps::board_post_reaction::delete,
    crate::controller::bps::board_post_report::list,
    crate::controller::bps::board_post_report::get,
    crate::controller::bps::board_post_report::create,
    crate::controller::bps::board_post_report::delete,
    crate::controller::bps::board_file::list,
    crate::controller::bps::board_file::get,
    crate::controller::bps::board_file::upload,
    crate::controller::bps::board_file::delete,
  ),
  tags(
    (name = BOARD_TAG, description = "Boards — where posts live and what may be done in them"),
    (name = BOARD_GROUP_TAG, description = "The tree boards are listed under"),
    (name = BOARD_TOPIC_TAG, description = "The topics a board sorts its posts into"),
    (name = POST_TAG, description = "Articles, notices, comments and replies — one resource, four kinds"),
    (name = REACTION_TAG, description = "The marks readers leave on a post"),
    (name = REPORT_TAG, description = "Posts readers have flagged, and what moderation did with them"),
    (name = BOARD_FILE_TAG, description = "Files attached to a post"),
  ),
)]
struct BoardRoutes;

/// The membership routes, mounted only where `membership` is on.
#[cfg(feature = "membership")]
#[derive(OpenApi)]
#[openapi(
  paths(
    crate::controller::membership::plan::list,
    crate::controller::membership::plan::get,
    crate::controller::membership::plan::create,
    crate::controller::membership::plan::update,
    crate::controller::membership::plan::delete,
    crate::controller::membership::subscription::list,
    crate::controller::membership::subscription::get,
    crate::controller::membership::subscription::subscribe,
    crate::controller::membership::subscription::cancel,
    crate::controller::membership::subscription::renew,
    crate::controller::membership::subscription::set_auto_renew,
    crate::controller::membership::subscription::reactivate,
    crate::controller::membership::subscription::change_plan,
    crate::controller::membership::credit::balance,
    crate::controller::membership::credit::transactions,
    crate::controller::membership::credit::grant,
    crate::controller::membership::credit::consume,
    crate::controller::membership::credit::refund,
    crate::controller::membership::credit::reserve,
    crate::controller::membership::credit::reservations,
    crate::controller::membership::credit::capture,
    crate::controller::membership::credit::release,
    crate::controller::membership::credit::audit,
    crate::controller::membership::entitlement::check,
    crate::controller::membership::purchase::list,
    crate::controller::membership::payment::checkout,
    crate::controller::membership::payment::confirm,
    crate::controller::membership::payment::refund,
    crate::controller::membership::payment::webhook,
    crate::controller::membership::coupon::list,
    crate::controller::membership::coupon::create,
    crate::controller::membership::coupon::update,
    crate::controller::membership::coupon::delete,
    crate::controller::membership::coupon::grant,
    crate::controller::membership::coupon::revoke,
    crate::controller::membership::coupon::grant_bulk,
    crate::controller::membership::coupon::grants,
    crate::controller::membership::coupon::mine,
    crate::controller::membership::coupon::redeem,
  ),
  tags(
    (name = PLAN_TAG, description = "What may be subscribed to or bought, and on what terms"),
    (name = SUBSCRIPTION_TAG, description = "A subscription's life — start, renew, change, cancel, revive"),
    (name = CREDIT_TAG, description = "The credit ledger: balances, grants, holds and the audit behind them"),
    (name = ENTITLEMENT_TAG, description = "The one gate that answers whether an action may proceed"),
    (name = PURCHASE_TAG, description = "What was bought, and what it came to"),
    (name = PAYMENT_TAG, description = "Checkout, confirmation, refunds, and the provider's callback"),
    (name = COUPON_TAG, description = "Coupons — minting them, handing them out, and spending them"),
  ),
)]
struct MembershipRoutes;

/// The fragment, for a consumer to merge into its own document.
///
/// Merging keeps the consumer's `info` and takes these paths, components, security schemes and
/// tags. A consumer that mounts these routes under a prefix nests the fragment under the same
/// prefix rather than editing the paths.
pub fn fragment() -> OpenApiDocument {
  let mut document = Fragment::openapi();

  #[cfg(feature = "mail-form")]
  document.merge(MailFormRoutes::openapi());
  #[cfg(feature = "file")]
  document.merge(FileRoutes::openapi());
  #[cfg(feature = "board")]
  document.merge(BoardRoutes::openapi());
  #[cfg(feature = "membership")]
  document.merge(MembershipRoutes::openapi());

  // The filter descriptor is one generic handler serving every resource, so there is no per-resource
  // function for an annotation to sit beside. Each resource's copy of it is built here instead.
  for (mount, tag, resource) in [
    (user::MOUNT, USER_TAG, "user"),
    (organization::MOUNT, ORGANIZATION_TAG, "organization"),
    (role::MOUNT, ROLE_TAG, "role"),
    (ban::MOUNT, BAN_TAG, "ban"),
    (user_invitation::MOUNT, USER_INVITATION_TAG, "user_invitation"),
    (access_log::MOUNT, ACCESS_LOG_TAG, "access_log"),
    (api_key::MOUNT, API_KEY_TAG, "api_key"),
    (relation_tuple::MOUNT, RELATION_TUPLE_TAG, "relation_tuple"),
  ] {
    document
      .paths
      .paths
      .insert(format!("{mount}{DESCRIPTOR_PATH}"), filter_descriptor(tag, resource));
  }

  // The same, for the resources a build carries only where their feature is on.
  #[allow(unused_mut)]
  let mut optional: Vec<(&str, &str, &str)> = Vec::new();
  #[cfg(feature = "mail-form")]
  optional.push((mail_form::MOUNT, MAIL_FORM_TAG, "mail_form"));
  #[cfg(feature = "file")]
  optional.push((fs::file::MOUNT, FILE_TAG, "file"));
  #[cfg(feature = "board")]
  optional.extend([
    (bps::board::MOUNT, BOARD_TAG, "board"),
    (bps::board_group::MOUNT, BOARD_GROUP_TAG, "board_group"),
    (bps::board_topic::MOUNT, BOARD_TOPIC_TAG, "board_topic"),
    (bps::board_post::MOUNT, POST_TAG, "board_post"),
    (bps::board_post_reaction::MOUNT, REACTION_TAG, "board_post_reaction"),
    (bps::board_post_report::MOUNT, REPORT_TAG, "board_post_report"),
    (bps::board_file::MOUNT, BOARD_FILE_TAG, "board_file"),
  ]);
  #[cfg(feature = "membership")]
  optional.extend([
    (membership::plan::MOUNT, PLAN_TAG, "membership_plan"),
    (membership::subscription::MOUNT, SUBSCRIPTION_TAG, "subscription"),
    (membership::purchase::MOUNT, PURCHASE_TAG, "membership_purchase"),
    (membership::coupon::MOUNT, COUPON_TAG, "coupon"),
  ]);

  for (mount, tag, resource) in optional {
    document
      .paths
      .paths
      .insert(format!("{mount}{DESCRIPTOR_PATH}"), filter_descriptor(tag, resource));
  }

  document.paths.paths.insert(CAPABILITIES_PATH.to_owned(), capabilities());

  document
}

/// The three ways a caller proves who they are.
struct Credentials;

impl Modify for Credentials {
  fn modify(&self, document: &mut OpenApiDocument) {
    let components = document.components.get_or_insert_with(Default::default);

    components.add_security_scheme(
      scheme::SESSION,
      SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::with_description(
        DEFAULT_COOKIE_NAME,
        "The cookie a password sign-in leaves behind. The name is a deployment setting and this is \
         only what it is called when the deployment chooses nothing, so a client reads the name off \
         the `Set-Cookie` the sign-in answers with rather than assuming it.",
      ))),
    );

    components.add_security_scheme(
      scheme::BEARER,
      SecurityScheme::Http(
        HttpBuilder::new()
          .scheme(HttpAuthScheme::Bearer)
          .bearer_format("JWT")
          .description(Some(
            "An access token, as `Authorization: Bearer <token>`. Only an access token is taken \
             here; a refresh token is spent at the route that mints access tokens. A token is \
             refused once it is logged out, once the account changes its password, and once it \
             expires.",
          ))
          .build(),
      ),
    );

    components.add_security_scheme(
      scheme::API_KEY,
      SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::with_description(
        "Authorization",
        "An API key, as `Authorization: ApiKey <key>` — the whole header value including the \
         `ApiKey ` prefix. A key issued as secured carries only the grants given to the key itself; \
         a key issued unsecured acts with everything its owner may do.",
      ))),
    );
  }
}

/// `GET <resource>/filters`, which answers with what this deployment's copy of that list actually
/// accepts.
///
/// The keys, the operators, the sortable fields and the page ceiling are not repeated here. The
/// document states shapes and that route states values; keeping the values in both places splits
/// them the moment one moves, and the deployed server is the one that is right.
pub fn filter_descriptor(tag: &str, resource: &str) -> PathItem {
  let operation = OperationBuilder::new()
    .operation_id(Some(format!("{resource}_filter_descriptor")))
    .tag(tag)
    .summary(Some("The fields, operators and page size this list knows"))
    .description(Some(
      "The keys a filter may name and the operators each one takes, the keys the list may be \
       sorted by, and the caps on a page and on a condition tree. A client reads this when it \
       starts and builds its filter form from the answer.\n\nNaming a field here is not permission \
       to see it. Which rows come back is settled by the caller's grants before any filter is \
       applied, so this route can never enumerate a resource its caller could not list.",
    ))
    .securities(Some(guarded()))
    .response(
      "200",
      ResponseBuilder::new()
        .description("What this deployment accepts")
        .content("application/json", ContentBuilder::new().build())
        .build(),
    )
    .response("401", refused("No credentials, or credentials the server will not take"))
    .response("403", refused("Credentials that carry no grant to read this resource"))
    .response("501", refused("An `Authorization` scheme other than `Bearer` or `ApiKey`"))
    .build();

  PathItem::new(HttpMethod::Get, operation)
}

/// The path the router mounts the capability list at. It is not a controller's, so the operation is
/// built here beside the assembly rather than beside a handler.
const CAPABILITIES_PATH: &str = "/capabilities";

/// `GET /capabilities` — which optional features this binary was built with.
///
/// It takes no credentials at all: what it answers is what this build *has*, never what the caller
/// may do. A client reads it to leave out a screen the server has no routes for, instead of
/// discovering that by a 404.
fn capabilities() -> PathItem {
  let operation = OperationBuilder::new()
    .operation_id(Some("capabilities"))
    .tag(CAPABILITY_TAG)
    .summary(Some("The optional features this build carries"))
    .description(Some(
      "One entry per feature compiled in. A feature that is absent is a set of routes this \
       deployment does not serve, so a client that reads this first can stop offering what is not \
       there.",
    ))
    .securities(Some(vec![utoipa::openapi::security::SecurityRequirement::default()]))
    .response(
      "200",
      ResponseBuilder::new()
        .description("The features this build carries")
        .content("application/json", ContentBuilder::new().build())
        .build(),
    )
    .build();

  PathItem::new(HttpMethod::Get, operation)
}

/// What a hand-built operation says about credentials: any one of the three ways in.
fn guarded() -> Vec<utoipa::openapi::security::SecurityRequirement> {
  use utoipa::openapi::security::SecurityRequirement;

  vec![
    SecurityRequirement::new::<&str, [&str; 0], &str>(scheme::SESSION, []),
    SecurityRequirement::new::<&str, [&str; 0], &str>(scheme::BEARER, []),
    SecurityRequirement::new::<&str, [&str; 0], &str>(scheme::API_KEY, []),
  ]
}

/// A refusal from a hand-built operation. It points at the same body the derived ones point at.
fn refused(description: &str) -> Response {
  ResponseBuilder::new()
    .description(description)
    .content(
      "application/json",
      ContentBuilder::new().schema(Some(Ref::from_schema_name("ErrorBody"))).build(),
    )
    .build()
}


// The error layer builds its body as a value rather than as a type, so there is no type here to
// hang a derive on. What stands in its place is mirrored below and held against a real response by
// `the_error_body_is_the_one_the_server_writes`.

/// The body every failure that reaches the error layer is written as.
///
/// The same keys come back whether a guard turned the request away or a handler answered it.
#[derive(ToSchema)]
#[schema(rename_all = "camelCase")]
pub struct ErrorBody {
  /// What a client branches on. An `E` followed by four digits, stable across builds.
  pub code:        String,
  /// The same number as the status line.
  pub status_code: u16,
  /// One line about what went wrong, for a person to read. It is written **only in debug builds** —
  /// a release build leaves the key out entirely, because the line can carry internal detail.
  ///
  /// So it is optional in every build of this document: calling it required would be false in
  /// release and dropping it would be false in development. A client must not branch on it; that is
  /// `code`'s job.
  #[schema(required = false)]
  pub reason:      String,
}

/// What a route that answers with nothing in particular sends: an empty object, carrying neither
/// what was changed nor what was removed.
#[derive(ToSchema)]
pub struct EmptyObject {}

/// The ways a request is turned away before it reaches what it asked for.
#[derive(IntoResponses)]
pub enum Refused {
  /// No credentials, credentials the server will not take, or an account that may no longer be let
  /// in — disabled, banned, or one whose password was changed after the credential was handed out.
  #[response(status = 401, content_type = "application/json")]
  NoCredentials(ErrorBody),
  /// Credentials that carry no grant for this route, or none for the row it names.
  #[response(status = 403, content_type = "application/json")]
  NoPermission(ErrorBody),
  /// An `Authorization` header naming a scheme this server does not implement. Only `Bearer` and
  /// `ApiKey` are read; anything else is refused rather than ignored.
  #[response(status = 501, content_type = "application/json")]
  UnknownScheme(ErrorBody),
}

/// The two ways the credential guard alone turns a request away.
///
/// A route that carries no grant check of its own — everything on the authentication surface, where
/// the question is only who is asking — cannot answer `403`, so it does not claim it may.
#[derive(IntoResponses)]
pub enum Unauthenticated {
  /// No credentials, credentials the server will not take, or an account that may no longer be let
  /// in — disabled, banned, or one whose credentials were reset after this one was handed out.
  #[response(status = 401, content_type = "application/json")]
  NoCredentials(ErrorBody),
  /// An `Authorization` header naming a scheme this server does not implement. Only `Bearer` and
  /// `ApiKey` are read; anything else is refused rather than ignored.
  #[response(status = 501, content_type = "application/json")]
  UnknownScheme(ErrorBody),
}

/// Nothing is stored under that identifier. A row the caller may not see answers the same way — an
/// existence a caller has no grant for is not disclosed by the status code.
#[derive(IntoResponses)]
#[response(status = 404, content_type = "application/json")]
pub struct NotFound(pub ErrorBody);

// The two below are refusals that never reach the error layer. What writes them is axum's own
// reader, whose wording moves independently of this crate, so they are checked separately.

/// Some piece of the request is not of the shape it has to be — an identifier in the path, a value
/// in the query string, or the JSON syntax itself.
///
/// The reader refuses it, not the handler, so the body is not the error layer's. It is the one line
/// of plain text the reader leaves, and it carries no machine-readable code.
#[derive(IntoResponses)]
#[response(status = 400, content_type = "text/plain")]
pub struct Unreadable(pub String);

/// The JSON was read but is not the shape this operation takes. This too is the reader refusing,
/// so it too is plain text.
#[derive(IntoResponses)]
#[response(status = 422, content_type = "text/plain")]
pub struct WrongBody(pub String);

#[cfg(test)]
mod tests {
  use std::collections::{BTreeMap, BTreeSet};

  use serde_json::Value as JsonValue;

  use super::*;

  /// The methods a path item can carry, in the spelling the document uses.
  const METHODS: [&str; 5] = ["get", "post", "put", "patch", "delete"];

  /// The controller sources the checks below read, beside the prefix each one mounts at.
  ///
  /// A source is read as text because axum's router cannot be asked what it was given. What is put
  /// side by side is therefore the statement that builds a route and the statement that describes
  /// one, out of the same file — and both name the same path constant, so the comparison is between
  /// constant names rather than between two spellings of a path.
  ///
  /// It is a function rather than a table because half of it is conditional: a build with a feature
  /// off compiles neither those controllers nor their descriptions, and a check has to read the same
  /// set the binary carries.
  fn controllers() -> Vec<(&'static str, &'static str)> {
    #[allow(unused_mut)]
    let mut sources: Vec<(&'static str, &'static str)> = vec![
      (include_str!("controller/user.rs"), user::MOUNT),
      (include_str!("controller/organization.rs"), organization::MOUNT),
      (include_str!("controller/role.rs"), role::MOUNT),
      (include_str!("controller/ban.rs"), ban::MOUNT),
      (include_str!("controller/user_invitation.rs"), user_invitation::MOUNT),
      (include_str!("controller/user_verification.rs"), crate::controller::user_verification::MOUNT),
      (include_str!("controller/access_log.rs"), access_log::MOUNT),
      (include_str!("controller/api_key.rs"), api_key::MOUNT),
      (include_str!("controller/authz.rs"), crate::controller::authz::MOUNT),
      (include_str!("controller/relation_tuple.rs"), relation_tuple::MOUNT),
      (include_str!("controller/dashboard.rs"), crate::controller::dashboard::MOUNT),
      (
        include_str!("controller/dashboard_overview.rs"),
        crate::controller::dashboard_overview::MOUNT,
      ),
    ];

    #[cfg(feature = "mail-form")]
    sources.push((include_str!("controller/mail_form.rs"), mail_form::MOUNT));
    #[cfg(feature = "file")]
    sources.push((include_str!("controller/fs/file.rs"), fs::file::MOUNT));
    #[cfg(feature = "board")]
    sources.extend([
      (include_str!("controller/bps/board.rs"), bps::board::MOUNT),
      (include_str!("controller/bps/board_group.rs"), bps::board_group::MOUNT),
      (include_str!("controller/bps/board_topic.rs"), bps::board_topic::MOUNT),
      (include_str!("controller/bps/board_post.rs"), bps::board_post::MOUNT),
      (
        include_str!("controller/bps/board_post_reaction.rs"),
        bps::board_post_reaction::MOUNT,
      ),
      (
        include_str!("controller/bps/board_post_report.rs"),
        bps::board_post_report::MOUNT,
      ),
      (include_str!("controller/bps/board_file.rs"), bps::board_file::MOUNT),
    ]);
    #[cfg(feature = "membership")]
    sources.extend([
      (include_str!("controller/membership/plan.rs"), membership::plan::MOUNT),
      (
        include_str!("controller/membership/subscription.rs"),
        membership::subscription::MOUNT,
      ),
      (include_str!("controller/membership/credit.rs"), membership::credit::MOUNT),
      (
        include_str!("controller/membership/entitlement.rs"),
        membership::entitlement::MOUNT,
      ),
      (include_str!("controller/membership/purchase.rs"), membership::purchase::MOUNT),
      (include_str!("controller/membership/payment.rs"), membership::payment::MOUNT),
      (include_str!("controller/membership/coupon.rs"), membership::coupon::MOUNT),
    ]);

    sources
  }

  /// Whether a path stands under one of the prefixes [`controllers`] names.
  ///
  /// The checks that read the assembled document answer for the operations that table lists, so a
  /// controller joins those checks by joining the table.
  fn described_here(path: &str) -> bool {
    owner_of(path).is_some()
  }

  /// Which prefix a path belongs to: the **longest** one it stands under.
  ///
  /// One prefix can lie inside another — the host monitoring is nested a level below the overview
  /// whose first segment it shares — and a path counted under both would make the count that holds
  /// a controller to its own routes say nothing.
  fn owner_of(path: &str) -> Option<&'static str> {
    controllers()
      .into_iter()
      .map(|(_, mount)| mount)
      .filter(|mount| path == *mount || path.starts_with(&format!("{mount}/")))
      .max_by_key(|mount| mount.len())
  }

  /// The argument list of a call, given the text just past its opening parenthesis.
  ///
  /// A parenthesis inside a string is prose and is not counted, because a description is free to
  /// carry one and a description that carried an odd number of them would otherwise cut the
  /// argument list short.
  fn arguments(text: &str) -> &str {
    let mut depth = 1usize;
    let mut quoted = false;
    let mut escaped = false;

    for (at, character) in text.char_indices() {
      if escaped {
        escaped = false;
        continue;
      }

      match character {
        '\\' if quoted => escaped = true,
        '"' => quoted = !quoted,
        '(' if !quoted => depth += 1,
        ')' if !quoted => {
          depth -= 1;
          if depth == 0 {
            return &text[..at];
          }
        }
        _ => {}
      }
    }

    panic!("a call that is never closed");
  }

  /// The collection sits at the prefix itself in the document and at `/` in the router. The two
  /// spellings are the same place, so they are read as one.
  fn normalize(path: &str) -> String {
    match path.trim() {
      "\"\"" | "\"/\"" => "\"/\"".to_owned(),
      other => other.to_owned(),
    }
  }

  /// The (method, path) pairs a controller's router builds.
  fn wired(source: &str) -> BTreeSet<(String, String)> {
    let mut built = BTreeSet::new();

    for at in source.match_indices(".route(").map(|(at, _)| at + ".route(".len()) {
      let call = arguments(&source[at..]);
      let (path, handlers) = call.split_once(',').expect("a route names a path and a handler");

      for method in METHODS {
        // The first handler is spelled `routing::get(...)`, and one chained onto it `.get(...)`.
        if handlers.contains(&format!("routing::{method}(")) || handlers.contains(&format!(".{method}("))
        {
          built.insert((method.to_owned(), normalize(path)));
        }
      }
    }

    built
  }

  /// The (method, path) pairs a controller's annotations describe.
  fn described(source: &str) -> BTreeSet<(String, String)> {
    let mut stated = BTreeSet::new();

    for chunk in source.split("utoipa::path(").skip(1) {
      let annotation = arguments(chunk);
      let method = annotation.split(',').next().expect("an operation names a method").trim();
      let path = annotation
        .split(',')
        .find_map(|argument| argument.trim().strip_prefix("path ="))
        .expect("an operation names a path");

      assert!(METHODS.contains(&method), "`{method}` is not a method this document writes");
      stated.insert((method.to_owned(), normalize(path)));
    }

    stated
  }

  /// A route that gains a description it does not have, or keeps one after it is gone, is caught
  /// here.
  #[test]
  fn every_route_is_described_and_every_description_has_a_route() {
    for (source, mount) in controllers() {
      let wired = wired(source);

      assert!(!wired.is_empty(), "`{mount}`: no route was read, so the scan and the source disagree");
      assert_eq!(wired, described(source), "`{mount}`");
    }
  }

  /// An annotation sits beside a handler rather than inside it, so it can ask for a body the handler
  /// never reads and hide one it does. Both are the same mistake and both are caught by putting the
  /// annotation and the signature it sits on side by side.
  #[test]
  fn a_body_is_described_exactly_where_a_body_is_read() {
    let mut seen = 0;

    for (source, mount) in controllers() {
      for chunk in source.split("utoipa::path(").skip(1) {
        let annotation = arguments(chunk);
        let rest = &chunk[annotation.len()..];
        let signature = rest.split_once(" {\n").expect("a handler opens a body").0;
        let name = signature.rsplit_once("async fn ").expect("a handler is a function").1;
        let name = name.split_once('<').map_or(name, |(name, _)| name);

        // Four readers take a body: `Json(..)` destructures one, `Multipart` walks it one part at a
        // time, `Bytes` hands the bytes over untouched so a signature can be checked against exactly
        // what arrived, and `Request` carries the whole thing to a reader chosen inside the handler.
        // Everything else a handler takes — the state, the session, a path segment, a query, the
        // headers — is read from somewhere other than the body.
        //
        // Only the parameters are searched. A `Json<..>` given back names the same type without
        // reading anything, and the same is true of a `Request` named in a return.
        let parameters = signature.split_once('(').expect("a function takes a list").1;
        let parameters = parameters.rsplit_once(')').expect("that list closes").0;

        let reads_a_body = ["Json(", "Multipart", "Bytes", "Request"]
          .iter()
          .any(|reader| parameters.contains(reader));

        assert_eq!(
          annotation.contains("request_body"),
          reads_a_body,
          "`{mount}` `{name}`: the description and the signature disagree about a body"
        );

        seen += 1;
      }
    }

    assert!(seen > 0, "no operation was read, so the scan and the sources disagree");
  }

  /// An annotation can be written and then left out of the assembly, which neither check above can
  /// see. Counting what each prefix mounts against what the document carries under it closes that.
  #[test]
  fn the_document_carries_every_route_each_controller_mounts() {
    let document = serde_json::to_value(fragment()).expect("the fragment is JSON");
    let paths = document["paths"].as_object().expect("a path table");

    for (source, mount) in controllers() {
      // One route per controller carries no annotation of its own: the filter descriptor is a
      // generic handler, and its operation is built by hand instead.
      // Two spellings mount the descriptor: through the action bundle, and merged straight into a
      // branch that already carries the gate the list stands behind.
      let mounted = wired(source).len()
        + source.matches(".descriptor::<").count()
        + source.matches("filter_descriptor_route::<").count();
      let documented: usize = paths
        .iter()
        .filter(|(path, _)| owner_of(path) == Some(mount))
        .map(|(_, item)| METHODS.iter().filter(|method| item.get(**method).is_some()).count())
        .sum();

      assert_eq!(mounted, documented, "`{mount}`");
    }
  }

  /// Every failure the document declares points at the shape the server really writes.
  ///
  /// The body itself is held against a real response by the check below; where that body was hung
  /// in the document is written by hand, so every failure response is walked and its media type and
  /// reference read back.
  #[test]
  fn every_failure_the_document_declares_names_what_the_server_sends() {
    let document = serde_json::to_value(fragment()).expect("the fragment is JSON");

    for (path, item) in document["paths"].as_object().expect("a path table") {
      if !described_here(path) {
        continue;
      }

      for method in METHODS {
        let Some(operation) = item.get(method) else { continue };

        for (status, response) in operation["responses"].as_object().expect("a response table") {
          if status.parse::<u16>().expect("a status is a number") < 400 {
            continue;
          }

          let Some(content) = response.get("content").and_then(JsonValue::as_object) else {
            continue;
          };

          for (media, body) in content {
            let at = format!("{method} {path} → {status} ({media})");

            match media.as_str() {
              // What the error layer writes. There is one shape, and it is checked below.
              "application/json" => assert_eq!(
                body["schema"]["$ref"],
                JsonValue::from("#/components/schemas/ErrorBody"),
                "{at}: points at something other than the failure body"
              ),
              // What a reader writes when it will not read something. One line of plain text.
              "text/plain" => assert_eq!(
                body["schema"]["type"],
                JsonValue::from("string"),
                "{at}: is not plain text"
              ),
              other => panic!("{at}: nothing answers a failure as `{other}`"),
            }
          }
        }
      }
    }
  }

  /// A name the document points at and defines nowhere.
  ///
  /// A wrapper used as a query parameter is where this bites: a parameter writes a `$ref` without
  /// registering what it refers to, so forgetting to spell the schema out leaves the reference
  /// hanging.
  #[test]
  fn no_reference_in_the_document_dangles() {
    fn collect(value: &JsonValue, into: &mut BTreeSet<String>) {
      match value {
        JsonValue::Object(map) => {
          for (key, child) in map {
            if key == "$ref" {
              if let Some(name) = child.as_str().and_then(|reference| reference.rsplit('/').next()) {
                into.insert(name.to_owned());
              }
            }
            collect(child, into);
          }
        }
        JsonValue::Array(items) => items.iter().for_each(|item| collect(item, into)),
        _ => {}
      }
    }

    let document = serde_json::to_value(fragment()).expect("the fragment is JSON");
    let defined: BTreeSet<String> = document["components"]["schemas"]
      .as_object()
      .map(|schemas| schemas.keys().cloned().collect())
      .unwrap_or_default();

    let mut referenced = BTreeSet::new();
    collect(&document, &mut referenced);

    let dangling: Vec<&String> = referenced.difference(&defined).collect();
    assert!(dangling.is_empty(), "nothing defines: {dangling:?}");
  }

  /// Every scheme an operation asks for is one the fragment defines.
  ///
  /// An operation names its schemes as bare strings, so a rename in the one place they are defined
  /// would otherwise leave the operations pointing at nothing at all.
  #[test]
  fn every_operation_asks_for_a_scheme_the_fragment_defines() {
    let document = serde_json::to_value(fragment()).expect("the fragment is JSON");
    let defined: BTreeSet<String> = document["components"]["securitySchemes"]
      .as_object()
      .map(|schemes| schemes.keys().cloned().collect())
      .unwrap_or_default();

    assert_eq!(
      defined,
      BTreeSet::from([
        scheme::SESSION.to_owned(),
        scheme::BEARER.to_owned(),
        scheme::API_KEY.to_owned(),
      ])
    );

    for (path, item) in document["paths"].as_object().expect("a path table") {
      for method in METHODS {
        let Some(operation) = item.get(method) else { continue };
        let Some(requirements) = operation.get("security").and_then(JsonValue::as_array) else {
          continue;
        };

        for requirement in requirements {
          for name in requirement.as_object().expect("a requirement names schemes").keys() {
            assert!(defined.contains(name), "{method} {path}: no scheme named `{name}`");
          }
        }
      }
    }
  }

  /// Two operations under one name make a document a code generator cannot read, so no operation
  /// these checks answer for shares its name with anything else in the fragment.
  #[test]
  fn no_operation_shares_its_name_with_another() {
    let document = serde_json::to_value(fragment()).expect("the fragment is JSON");
    let mut everywhere: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for (path, item) in document["paths"].as_object().expect("a path table") {
      for method in METHODS {
        let Some(operation) = item.get(method) else { continue };
        let Some(name) = operation.get("operationId").and_then(JsonValue::as_str) else {
          continue;
        };

        everywhere.entry(name.to_owned()).or_default().push(format!("{method} {path}"));
      }
    }

    for (path, item) in document["paths"].as_object().expect("a path table") {
      if !described_here(path) {
        continue;
      }

      for method in METHODS {
        let Some(operation) = item.get(method) else { continue };
        let name = operation["operationId"].as_str().expect("an operation is named");
        let at = &everywhere[name];

        assert_eq!(at.len(), 1, "`{name}` names more than one operation: {at:?}");
      }
    }
  }

  /// The keys a schema names.
  fn properties<T: utoipa::PartialSchema>() -> BTreeSet<String> {
    let schema = serde_json::to_value(T::schema()).expect("a schema is JSON");

    schema["properties"].as_object().expect("an object schema").keys().cloned().collect()
  }

  /// The keys a schema calls required.
  fn required<T: utoipa::PartialSchema>() -> BTreeSet<String> {
    let schema = serde_json::to_value(T::schema()).expect("a schema is JSON");

    schema["required"]
      .as_array()
      .map(|keys| keys.iter().filter_map(|key| key.as_str().map(str::to_owned)).collect())
      .unwrap_or_default()
  }

  /// The failure body written out here is the one the error layer really sends — the media type,
  /// the keys, the types of their values, and which of them may be missing.
  ///
  /// A guard and a handler both put a failure on the wire through the one `IntoResponse`, so what
  /// is compared here is how the whole surface answers a failure.
  ///
  /// `reason` is written only in debug builds. The document calls it optional and says the same
  /// thing whichever build wrote it, so this is the one place the two builds are told apart.
  #[tokio::test]
  async fn the_error_body_is_the_one_the_server_writes() {
    use axum::http::StatusCode;
    use axum::http::header::CONTENT_TYPE;
    use axum::response::IntoResponse;
    use error::SumidaError;

    // What a guard writes and what a handler writes, with a reason and without a row.
    let failures = [
      SumidaError::NoCredentials,
      SumidaError::NoAccessPermission,
      SumidaError::NoServiceAvailable,
      SumidaError::InvalidAuthorizationMethod("Basic".to_owned()),
      SumidaError::NoData("UserInvitation".to_owned(), "x".to_owned()),
      SumidaError::BadRequest("the tree steps outside the declaration".to_owned()),
    ];

    let named = properties::<ErrorBody>();
    let required = required::<ErrorBody>();

    for failure in failures {
      let status = StatusCode::from(failure.clone());
      let response = failure.clone().into_response();

      assert_eq!(response.status(), status);
      assert_eq!(
        response.headers().get(CONTENT_TYPE).map(|media| media.as_bytes()),
        Some(&b"application/json"[..]),
        "`{failure}` is written as something other than what the document says"
      );

      let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.expect("a body");
      let written: JsonValue = serde_json::from_slice(&body).expect("the body is JSON");
      let sent: BTreeSet<String> =
        written.as_object().expect("an object").keys().cloned().collect();

      assert!(sent.is_subset(&named), "a key the document does not name is written: {sent:?}");
      assert!(required.is_subset(&sent), "a key the document requires is missing: {sent:?}");

      assert!(written["code"].is_string(), "`code` is a string");
      assert_eq!(written["statusCode"].as_u64(), Some(u64::from(status.as_u16())));

      assert_eq!(
        sent.contains("reason"),
        cfg!(debug_assertions),
        "`reason` is written in debug builds only, which is why the document calls it optional"
      );
    }
  }

  /// The two refusals the readers write are plain text, which is why the document does not call
  /// them the error layer's body.
  ///
  /// The reader is axum's, not this crate's, so its wording moves on its own. A change there makes
  /// two statements in the document false at once, and it is caught here.
  #[tokio::test]
  async fn what_the_readers_refuse_comes_back_as_plain_text() {
    use axum::body::Body;
    use axum::extract::Query;
    use axum::http::header::{CONTENT_TYPE, HeaderValue};
    use axum::http::{Request, StatusCode};
    use axum::{Json, Router, routing};
    use tower::ServiceExt;

    use crate::service::user::dto::{CreateUser, UserFilter};

    let router = Router::new()
      .route("/body", routing::post(|Json(_): Json<CreateUser>| async {}))
      .route("/query", routing::get(|Query(_): Query<UserFilter>| async {}))
      .route("/{id}", routing::get(|axum::extract::Path(_): axum::extract::Path<i32>| async {}));

    let posted = |body: &'static str| {
      Request::builder()
        .method("POST")
        .uri("/body")
        .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Body::from(body))
        .expect("a request")
    };
    let got = |uri: &str| Request::builder().uri(uri).body(Body::empty()).expect("a request");

    let cases = [
      // Not JSON at all.
      (posted("{"), StatusCode::BAD_REQUEST),
      // JSON, but not the shape this operation takes.
      (posted("{}"), StatusCode::UNPROCESSABLE_ENTITY),
      // A query-string value that cannot be read.
      (got("/query?is_admin=perhaps"), StatusCode::BAD_REQUEST),
      // A path segment that cannot be read.
      (got("/not-a-number"), StatusCode::BAD_REQUEST),
    ];

    for (request, expected) in cases {
      let response = router.clone().oneshot(request).await.expect("a router does not fail");

      assert_eq!(response.status(), expected);

      let media = response.headers().get(CONTENT_TYPE).expect("a media type");
      assert!(
        media.to_str().expect("a media type is text").starts_with("text/plain"),
        "a reader refuses with `{media:?}` rather than plain text"
      );
    }
  }

  /// A handler that asks for a path parameter its route does not carry is refused by the reader
  /// with a `500`, before the handler runs — which is what two of the invitation operations say
  /// about themselves.
  #[tokio::test]
  async fn a_path_parameter_a_route_does_not_carry_is_a_server_error() {
    use axum::body::Body;
    use axum::extract::Path;
    use axum::http::header::CONTENT_TYPE;
    use axum::http::{Request, StatusCode};
    use axum::{Router, routing};
    use tower::ServiceExt;

    let router = Router::new().route("/", routing::post(|Path(_): Path<i32>| async {}));
    let response = router
      .oneshot(Request::builder().method("POST").uri("/").body(Body::empty()).expect("a request"))
      .await
      .expect("a router does not fail");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let media = response.headers().get(CONTENT_TYPE).expect("a media type");
    assert!(media.to_str().expect("a media type is text").starts_with("text/plain"));
  }

  /// Every path the document names is a path the router really serves.
  ///
  /// The checks above read source text; this one puts a request into the assembled router, so a
  /// path spelled in a syntax axum does not read, or hung under the wrong prefix, comes back a 404.
  ///
  /// Methods are not told apart this way: the credential guard answers before a method ever
  /// mismatches.
  #[tokio::test]
  async fn every_path_the_document_names_is_served() {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use sea_orm::DatabaseConnection;
    use tower::ServiceExt;

    use crate::SumidaState;
    use crate::controller::ApiController;

    #[derive(Clone)]
    struct ProbeState(DatabaseConnection);

    impl SumidaState for ProbeState {
      fn db(&self) -> &DatabaseConnection {
        // A request is turned away for want of credentials before any query runs, and a broken
        // connection answers with an error rather than ending the check in a panic.
        &self.0
      }
    }

    let state = ProbeState(DatabaseConnection::Disconnected);
    // The router the consumer really mounts, rather than one assembled here: a prefix that drifted
    // between the assembly and the router would otherwise stay invisible to this check.
    // An empty rule set is what a product with no rules of its own declares, and installing it
    // twice with the same slice is not an error.
    let router = crate::Sumida::new(&[]).set_state(&state).router().with_state(state);

    for path in fragment().paths.paths.keys() {
      if !described_here(path) {
        continue;
      }

      let target = path
        .replace("{user_id}", "1")
        .replace("{id}", "1")
        .replace("{kind}", "welcome")
        .replace("{uuid}", "0195b0a1-0000-7000-8000-000000000000");
      let response = router
        .clone()
        .oneshot(Request::builder().uri(&target).body(Body::empty()).expect("a request"))
        .await
        .expect("a router does not fail");

      assert_ne!(response.status(), StatusCode::NOT_FOUND, "nothing serves `{path}`");
    }
  }
}

