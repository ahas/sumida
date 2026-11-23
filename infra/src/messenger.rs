use axum::extract::FromRequestParts;
use error::SumidaError;
use sea_orm::DatabaseConnection;
use socketioxide::extract::{SocketRef, State};
use socketioxide::handler::ConnectHandler;
use socketioxide::layer::SocketIoLayer;
use socketioxide::{BroadcastError, SocketIo};
use socketioxide_core::adapter::{Room, RoomParam};

use crate::origin::{allowed_origins, source_allowed};
use crate::session::{UserSession, admitted_account, credentials_reset_since};

/// What the socket handshake needs to admit a connection: the database the account is re-read from.
#[derive(Clone, Debug)]
pub struct SumidaMessengerState {
  pub db: DatabaseConnection,
}

#[derive(Clone, Debug)]
pub struct SumidaMessenger {
  pub io:    SocketIo,
  pub path:  Option<String>,
  pub rooms: Option<Vec<Room>>,
}

impl SumidaMessenger {
  pub fn new(io: SocketIo) -> Self {
    Self {
      io,
      path: None,
      rooms: None,
    }
  }

  #[inline]
  pub fn of(&self, path: impl ToString) -> Self {
    let mut messenger = self.clone();
    messenger.path = Some(path.to_string());
    messenger
  }

  #[inline]
  pub fn within(&self, rooms: impl RoomParam) -> Self {
    let rooms: Vec<_> = rooms.into_room_iter().collect();

    let mut messenger = self.clone();
    messenger.rooms = Some(match messenger.rooms.take() {
      Some(already) => [already, rooms].concat(),
      None => rooms,
    });

    messenger
  }

  #[inline]
  pub fn org(&self, org_ids: impl IntoIterator<Item = i32>) -> Self {
    let org_rooms = org_ids.into_iter().map(|x| format!("org:{x}")).collect::<Vec<_>>();
    self.within(org_rooms)
  }

  #[inline]
  pub fn user(&self, user_ids: impl IntoIterator<Item = i32>) -> Self {
    let user_rooms = user_ids.into_iter().map(|x| format!("user:{x}")).collect::<Vec<_>>();
    self.within(user_rooms)
  }

  #[inline]
  pub async fn emit<T>(&self, event: impl AsRef<str>, data: &T) -> Result<(), BroadcastError>
  where
    T: ?Sized + serde::Serialize,
  {
    let path = if let Some(ref path) = self.path { path.as_str() } else { "/" };
    let Some(io) = self.io.of(path) else {
      // If the namespace is not registered, there is no target to send to. Silently ignore instead of panicking.
      tracing::warn!("messenger namespace not found: {path}");
      return Ok(());
    };
    let io = if let Some(rooms) = self.rooms.clone() { io.within(rooms) } else { io };
    // Every emit is also broadcast to the admin room (for admin monitoring; intended behavior).
    // Even org/user-only messages reach admins, so use a dedicated channel for sensitive payloads.
    let io = io.within("admin");

    io.emit(event, data).await?;

    Ok(())
  }
}

pub async fn get_session(
  socket: &SocketRef,
  state: &SumidaMessengerState,
) -> Result<UserSession, SumidaError> {
  let mut req_parts = socket.req_parts().to_owned();

  UserSession::from_request_parts(&mut req_parts, state).await
}

/// Admits a socket connection to `/user` and joins it to its rooms, or refuses it.
///
/// It runs before the connection is accepted, so a refusal reaches the client as a connection
/// error. The handshake is held to what every HTTP request is held to: the cookie must be
/// authorized, the account it names must still be enabled and not banned, and its credentials must
/// not have been reset since the cookie was issued. The administrator room is decided from the
/// account as it is now, never from the flag the cookie copied at sign-in, and never for a key that
/// carries only its own grants.
pub async fn admit(
  socket: SocketRef,
  State(state): State<SumidaMessengerState>,
) -> Result<(), SumidaError> {
  // CORS does not apply to a WebSocket upgrade, so this is the only thing that keeps another site's
  // page from opening a socket with the visitor's cookie.
  if !source_allowed(socket.req_parts(), allowed_origins()) {
    return Err(SumidaError::NoAccessPermission);
  }

  let session = get_session(&socket, &state).await?;
  session.verify()?;

  let user = admitted_account(session.profile.id, &session.env, &state.db).await?;
  if credentials_reset_since(&user, session.profile.first_seen_at) {
    return Err(SumidaError::NoCredentials);
  }

  let profile = session.profile;
  socket.join([
    format!("org:{}", profile.organization.id),
    format!("role:{}", profile.role.id),
    // user rooms are keyed by user_id. `messenger.user([id])` emit sends to `user:{id}`, so
    // join must also use the id, not the username, for targeted notifications to arrive.
    format!("user:{}", profile.id),
  ]);

  if user.is_admin && profile.api_key.is_none() {
    socket.join("admin");
  }

  Ok(())
}

/// The connection handler. Everything a connection needs was settled by [`admit`] before it was
/// accepted.
pub async fn on_connect(socket: SocketRef) {
  tracing::debug!("socket connected: {}", socket.id);
}

/// The socket layer and the messenger that emits through it.
///
/// `db` is what each handshake re-reads its account from, so a connection is admitted on the same
/// terms as an HTTP request.
pub fn new_layer(db: DatabaseConnection) -> (SocketIoLayer, SumidaMessenger) {
  let (layer, io) = SocketIo::builder()
    .with_state(SumidaMessengerState { db })
    // Enlarge the per-socket send buffer (default 128). This reduces emits (e.g. term:open)
    // being dropped as `InternalChannelFull` due to transient backpressure.
    .max_buffer_size(1024)
    // Detect/clean up dead (half-open) sockets faster (default 25s/20s → takes ~45s).
    // This reduces the time a zombie socket lingers in a room and messages sent there are dropped.
    .ping_interval(std::time::Duration::from_secs(10))
    .ping_timeout(std::time::Duration::from_secs(10))
    .build_layer();

  io.ns("/user", on_connect.with(admit));

  (layer, SumidaMessenger::new(io))
}
