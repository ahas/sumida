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
  let same_site = same_site(settings.same_site.as_deref())?;

  let mut layer = SessionManagerLayer::new(store).with_name(name).with_secure(secure);

  if let Some(domain) = domain {
    layer = layer.with_domain(domain);
  }
  if let Some(path) = path {
    layer = layer.with_path(path);
  }

  Ok(layer.with_same_site(same_site))
}

/// The session cookie's `SameSite` attribute, read from `session.cookie.same_site`.
///
/// Left unset it is `Lax`: a cross-site page can still link to the service, but a form it submits
/// or a request it sends in the background arrives without the session cookie, so none of the
/// service's state-changing routes can be driven from another site on a visitor's behalf. `None` is
/// for a deployment whose pages are served from another site, and is only ever what the settings say
/// in so many words. A value that is none of the three is refused at startup, because guessing at a
/// misspelt one has to fall to one side, and either side is a surprise.
pub fn same_site(value: Option<&str>) -> Result<SameSite, anyhow::Error> {
  let Some(value) = value else {
    return Ok(SameSite::Lax);
  };

  match value.trim().to_ascii_lowercase().as_str() {
    "lax" => Ok(SameSite::Lax),
    "strict" => Ok(SameSite::Strict),
    "none" => Ok(SameSite::None),
    _ => Err(anyhow::anyhow!(
      "`session.cookie.same_site` is `{value}`, which is none of `lax`, `strict` or `none`"
    )),
  }
}

#[cfg(test)]
mod tests {
  use tower_sessions::cookie::SameSite;

  use super::same_site;

  #[test]
  fn an_unset_same_site_is_lax() {
    assert_eq!(same_site(None).unwrap(), SameSite::Lax);
  }

  #[test]
  fn same_site_is_read_without_regard_to_case() {
    assert_eq!(same_site(Some("Strict")).unwrap(), SameSite::Strict);
    assert_eq!(same_site(Some("LAX")).unwrap(), SameSite::Lax);
    assert_eq!(same_site(Some("none")).unwrap(), SameSite::None);
  }

  #[test]
  fn an_unknown_same_site_is_refused_rather_than_read_as_none() {
    assert!(same_site(Some("stirct")).is_err());
    assert!(same_site(Some("")).is_err());
  }
}
