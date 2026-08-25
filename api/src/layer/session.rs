use infra::settings;
use tower_sessions::cookie::SameSite;
use tower_sessions::{SessionManagerLayer, SessionStore};

/// The name of the session cookie when the deployment does not choose one.
///
/// The document describes the cookie a caller has to send back, and a caller cannot send back a
/// cookie whose name it does not know. So the name lives in one place and both the layer that sets
/// it and the description of it read that place.
pub const DEFAULT_COOKIE_NAME: &str = "sid";

pub async fn new<Store: SessionStore>(store: Store) -> Result<SessionManagerLayer<Store>, anyhow::Error> {
  let settings = &settings::get().session.cookie;
  let name = settings.name.clone().unwrap_or_else(|| DEFAULT_COOKIE_NAME.to_owned());
  let secure = settings.secure.unwrap_or(true);
  let domain = settings.domain.clone();
  let path = settings.path.clone();
  let same_site = settings.same_site.clone().unwrap_or("none".to_string());

  let mut layer = SessionManagerLayer::new(store).with_name(name).with_secure(secure);

  if let Some(domain) = domain {
    layer = layer.with_domain(domain);
  }
  if let Some(path) = path {
    layer = layer.with_path(path);
  }
  let layer = match same_site.as_str() {
    "lax" => layer.with_same_site(SameSite::Lax),
    "strict" => layer.with_same_site(SameSite::Strict),
    _ => layer.with_same_site(SameSite::None),
  };

  Ok(layer)
}
