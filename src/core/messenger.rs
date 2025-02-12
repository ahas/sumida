use axum::extract::FromRequestParts;
use socketioxide::extract::State;
use socketioxide::layer::SocketIoLayer;
use socketioxide::BroadcastError;
use socketioxide::{extract::SocketRef, SocketIo};
use socketioxide_core::adapter::{Room, RoomParam};

use crate::core::error::AppError;
use crate::service::user::session::UserSession;

#[derive(Clone, Debug)]
pub struct AppMessengerState;

#[derive(Clone, Debug)]
pub struct AppMessenger {
  pub io: SocketIo,
  pub path: Option<String>,
  pub rooms: Option<Vec<Room>>,
}

impl AppMessenger {
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
  pub fn org(&self, org_ids: impl RoomParam) -> Self {
    self.clone().within(org_ids.into_room_iter().map(|x| format!("org:{x}")).collect::<Vec<String>>())
  }

  #[inline]
  pub fn user(&self, usernames: impl RoomParam) -> Self {
    self.clone().within(usernames.into_room_iter().map(|x| format!("user:{x}")).collect::<Vec<String>>())
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

async fn get_session(socket: &SocketRef, state: &AppMessengerState) -> Result<UserSession, AppError> {
  let mut req_parts = socket.req_parts().to_owned();

  UserSession::from_request_parts(&mut req_parts, state).await
}

async fn on_connect(socket: SocketRef, State(state): State<AppMessengerState>) {
  tracing::info!("connect socket");

  let Some(session) = get_session(&socket, &state).await.ok() else {
    tracing::error!("socket session failed");
    socket.disconnect();
    return;
  };

  if session.verify().await.is_err() {
    tracing::error!("socket session verifying failed");
    socket.disconnect();
    return;
  }

  let user_data = session.user_data;

  socket.join([
    format!("org:{}", user_data.organization.id),
    format!("role:{}", user_data.role.id),
    format!("user:{}", user_data.username),
  ]);

  if user_data.is_admin {
    socket.join("admin");
  }
}

pub fn new_layer() -> (SocketIoLayer, AppMessenger) {
  let (layer, io) = SocketIo::builder().with_state(AppMessengerState).build_layer();

  io.ns("/user", on_connect);

  (layer, AppMessenger::new(io))
}
