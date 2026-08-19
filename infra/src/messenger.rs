use axum::extract::FromRequestParts;
use error::SumidaError;
use socketioxide::extract::{SocketRef, State};
use socketioxide::layer::SocketIoLayer;
use socketioxide::{BroadcastError, SocketIo};
use socketioxide_core::adapter::{Room, RoomParam};

use crate::session::UserSession;

#[derive(Clone, Debug)]
pub struct SumidaMessengerState;

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
    let rooms = rooms.into_room_iter().collect();

    let mut messenger = self.clone();
    messenger.rooms = Some(if messenger.rooms.is_none() {
      rooms
    } else {
      [messenger.rooms.unwrap(), rooms].concat()
    });

    messenger
  }

  #[inline]
  pub fn org(&self, org_ids: impl IntoIterator<Item = i32>) -> Self {
    let org_rooms = org_ids.into_iter().map(|x| format!("org:{x}")).collect::<Vec<_>>();
    self.clone().within(org_rooms)
  }

  #[inline]
  pub fn user(&self, user_ids: impl IntoIterator<Item = i32>) -> Self {
    let user_rooms = user_ids.into_iter().map(|x| format!("user:{x}")).collect::<Vec<_>>();
    self.clone().within(user_rooms)
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

pub async fn on_connect(socket: SocketRef, State(state): State<SumidaMessengerState>) {
  let Some(session) = get_session(&socket, &state).await.ok() else {
    tracing::error!("socket session failed");
    socket.disconnect().ok();
    return;
  };

  if let Err(err) = session.verify() {
    tracing::error!("socket session verifying failed: {}", err);
    socket.disconnect().ok();
    return;
  }

  let user_data = session.profile;

  socket.join([
    format!("org:{}", user_data.organization.id),
    format!("role:{}", user_data.role.id),
    // user rooms are keyed by user_id. `messenger.user([id])` emit sends to `user:{id}`, so
    // join must also use the id, not the username, for targeted notifications to arrive.
    format!("user:{}", user_data.id),
  ]);

  if user_data.is_admin {
    socket.join("admin");
  }
}

pub fn new_layer() -> (SocketIoLayer, SumidaMessenger) {
  let (layer, io) = SocketIo::builder()
    .with_state(SumidaMessengerState)
    // Enlarge the per-socket send buffer (default 128). This reduces emits (e.g. term:open)
    // being dropped as `InternalChannelFull` due to transient backpressure.
    .max_buffer_size(1024)
    // Detect/clean up dead (half-open) sockets faster (default 25s/20s → takes ~45s).
    // This reduces the time a zombie socket lingers in a room and messages sent there are dropped.
    .ping_interval(std::time::Duration::from_secs(10))
    .ping_timeout(std::time::Duration::from_secs(10))
    .build_layer();

  io.ns("/user", on_connect);

  (layer, SumidaMessenger::new(io))
}
