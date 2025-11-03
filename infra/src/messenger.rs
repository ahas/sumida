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
  pub io: SocketIo,
  pub path: Option<String>,
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
    let io = self.io.of(if let Some(ref path) = self.path { path.as_str() } else { "/" }).unwrap();
    let io = if let Some(rooms) = self.rooms.clone() { io.within(rooms) } else { io };
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
    format!("user:{}", user_data.username),
  ]);

  if user_data.is_admin {
    socket.join("admin");
  }
}

pub fn new_layer() -> (SocketIoLayer, SumidaMessenger) {
  let (layer, io) = SocketIo::builder().with_state(SumidaMessengerState).build_layer();

  io.ns("/user", on_connect);

  (layer, SumidaMessenger::new(io))
}
