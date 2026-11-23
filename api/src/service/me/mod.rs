pub mod dto;

use chrono::Utc;
use dto::*;
use entity::access_logs::{AccessMethod, AccessResultCode};
use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::ActiveValue::{NotSet, Set, Unchanged};
use sea_orm::{ActiveModelTrait, EntityTrait, IntoActiveModel, Select};
use sea_orm_crud::Optional;
use serde_json::json;

use crate::SumidaState;
use crate::service::auth::helper::verify_password;
use crate::service::user::UserService;
use crate::service::user_verification::UserVerificationService;
use crate::util::user::hash_password;

/// The verification code kind that proves a change of one's own password or email.
pub const ACCOUNT_CHANGE_KIND: &str = "account-change";

pub struct MeService<S: SumidaState> {
  state:   S,
  session: UserSession,
}

impl<S: SumidaState> MeService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  /// `/me` 는 언제나 세션 본인의 행이므로 행 스코프를 붙이지 않는다. 열·조인은
  /// `/users` 와 같은 것을 써야 응답 모양이 갈라지지 않으므로 `UserService` 의 것을 그대로 쓴다.
  fn default_query_with(query: Select<Users>) -> Select<Users> {
    UserService::<S>::default_query_with(query)
  }

  pub async fn get_session(&self) -> Result<Option<Me>, SumidaError> {
    if !self.session.profile.is_authorized {
      return Err(SumidaError::NoCredentials);
    }

    let Some(mut me_session) = Self::default_query_with(Users::find_by_id(self.session.profile.id))
      .into_model::<Me>()
      .one(self.state.db())
      .await?
    else {
      return Ok(None);
    };

    me_session.first_seen_at = self.session.profile.first_seen_at;
    me_session.last_seen_at = self.session.profile.last_seen_at;

    Ok(Some(me_session))
  }

  pub async fn update_me(&mut self, data: UpdateMe) -> Result<entity::users::Model, SumidaError> {
    let state = self.state.clone();
    let db = state.db();
    let user_id = self.session.profile.id;
    let username = self.session.profile.username.clone();
    let Optional(password) = data.password.clone();
    let Optional(current_password) = data.current_password.clone();
    let Optional(verification_code) = data.verification_code.clone();

    let mut model: entity::users::ActiveModel = data.into_active_model();
    model.id = Unchanged(user_id);
    model.username = Set(username.clone());

    let current = Users::find_by_id(user_id).one(db).await?.ok_or(SumidaError::InvalidAccount)?;
    let email_changes = matches!(&model.email, Set(email) if *email != current.email);

    // 비밀번호와 이메일은 계정을 넘겨주는 변경이다 — 비밀번호는 로그인이고 이메일은 재설정 코드가
    // 가는 곳이다. 그래서 요청이 들고 온 자격(잠깐 훔친 쿠키나 토큰일 수 있다) 말고 증명을 하나 더
    // 요구한다. 보안 키는 자기가 받은 것만 가지므로 계정 자체를 바꾸지 못한다.
    if password.is_some() || email_changes {
      if self.session.profile.api_key.is_some() {
        return Err(SumidaError::NoAccessPermission);
      }
      self.prove(&current, current_password, verification_code).await?;
    }

    if let Some(ref password) = password {
      model.password = Set(hash_password(password).map_err(|e| SumidaError::Unsupported(e.to_string()))?);
      // 새 비밀번호는 그전에 내준 세션과 토큰을 모두 무효로 만든다 — 바꾸는 이유가 그것이 남의 손에
      // 있을지 모른다는 것이기 때문이다.
      model.credentials_reset_at = Set(Some(Utc::now()));
    }

    model.role_id = NotSet;
    model.organization_id = NotSet;
    model.is_admin = NotSet;
    model.is_enabled = NotSet;

    self.session.identify(db).await?;

    let updated = Users::update(model).exec(db).await?;

    // 바꾼 사람 자신의 쿠키 세션은 재설정 뒤에 새로 세워서 로그인을 유지한다. 헤더로 들어온
    // 토큰은 다시 발급받아야 한다.
    if password.is_some() && !self.session.is_request_local().await {
      let expiry = self.session.profile.expiry;
      self.session.authorize(user_id, &username, expiry, db).await?;
    }

    Ok(updated)
  }

  /// 비밀번호·이메일 변경의 증명. 인증 코드(`account-change`)가 오면 그것을, 아니면 현재
  /// 비밀번호를 본다. 틀린 비밀번호는 로그인 실패와 같이 기록되고 같은 차단 규칙에 걸린다 —
  /// 그렇지 않으면 이 경로가 잠금 없는 비밀번호 확인 창구가 된다.
  async fn prove(
    &mut self,
    user: &entity::users::Model,
    current_password: Option<String>,
    verification_code: Option<String>,
  ) -> Result<(), SumidaError> {
    let state = self.state.clone();
    let db = state.db();

    if let Some(code) = verification_code {
      let verifier = UserVerificationService::<S>::new(self.state.clone(), self.session.clone());
      return if verifier.verify(ACCOUNT_CHANGE_KIND, user.id, &code, db).await? {
        Ok(())
      } else {
        Err(SumidaError::VerificationFailed)
      };
    }

    let Some(current_password) = current_password else {
      return Err(SumidaError::VerificationFailed);
    };
    if verify_password(&current_password, &user.password) {
      return Ok(());
    }

    self
      .session
      .write_log(Some(user.id), AccessMethod::Password, AccessResultCode::PasswordMismatch, db)
      .await?;
    let banned = self.session.ban_if_exceeded_limit(Some(user.id), db).await?;

    Err(if banned { SumidaError::AccountBanned } else { SumidaError::VerificationFailed })
  }

  pub async fn get_prefs(&self) -> Result<serde_json::Value, SumidaError> {
    let db = self.state.db();
    let pref = UserPrefs::find_by_id(self.session.profile.id).one(db).await?;

    Ok(pref.map(|x| x.data).unwrap_or_else(|| json!({})))
  }

  pub async fn update_prefs(&self, data: serde_json::Value) -> Result<(), SumidaError> {
    use entity::user_prefs::ActiveModel;

    let db = self.state.db();
    let user_id = self.session.profile.id;
    let pref = UserPrefs::find_by_id(user_id).one(db).await?;
    let mut am = ActiveModel {
      user_id: Set(user_id),
      data:    Set(data),
    };

    if pref.is_some() {
      am.user_id = Unchanged(user_id);
    }

    am.save(db).await?;

    Ok(())
  }
}
