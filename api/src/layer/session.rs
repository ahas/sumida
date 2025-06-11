use core::settings;
use tower_sessions::{cookie::SameSite, SessionManagerLayer, SessionStore};

pub async fn new<Store: SessionStore>(
  store: Store,
) -> Result<SessionManagerLayer<Store>, anyhow::Error> {
  let settings = &settings::get().session;
  let name = settings.name.clone().unwrap_or(String::from("sid"));
  let secure = settings.secure.unwrap_or(true);
  let domain = settings.domain.clone();
  let path = settings.path.clone();
  let same_site = settings.same_site.clone().unwrap_or("none".to_string());

  tracing::info!("session layer settings:");
  tracing::info!("- name: {name}");
  tracing::info!("- secure: {secure}");
  tracing::info!("- domain: {:?}", domain);

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
