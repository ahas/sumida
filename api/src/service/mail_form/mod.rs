pub mod dto;

use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use infra::settings::smtp::SmtpTlsMode;
use lettre::message::Message;
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::{SmtpTransport, Transport};
use sea_orm::{
  ConnectionTrait,
  DeleteResult,
  EntityTrait,
  InsertResult,
  IntoActiveModel,
  PaginatorTrait,
  QuerySelect,
  Select,
  Set,
  TransactionTrait,
};
use serde::Serialize;
use tinytemplate::TinyTemplate;

use self::dto::*;
use crate::SumidaState;
use crate::authz::Authz;
use crate::authz::subject::SubjectSet;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::Pagination;

/// 이 서비스가 판정하는 정의(`schema.sz`의 `namespace mail_form service = mail_form`).
const RESOURCE: &str = "mail_form";

pub struct MailFormSendOptions {
  pub username: String,
  pub password: String,
  pub auth:     String,
  pub server:   String,
  pub port:     u16,
  pub tls:      SmtpTlsMode,
}

impl MailFormSendOptions {
  pub fn into_smtp_transport(self) -> Result<SmtpTransport, anyhow::Error> {
    let cred = Credentials::new(self.username.clone(), self.password.clone());
    let auth = match self.auth.to_uppercase().as_str() {
      "PLAIN" => Mechanism::Plain,
      "LOGIN" => Mechanism::Login,
      "XOAUTH2" => Mechanism::Xoauth2,
      _ => Mechanism::Plain,
    };

    let builder = match self.tls {
      SmtpTlsMode::None => SmtpTransport::builder_dangerous(&self.server),
      SmtpTlsMode::Starttls => SmtpTransport::starttls_relay(&self.server)?,
      SmtpTlsMode::Implicit => SmtpTransport::relay(&self.server)?,
    };

    Ok(builder.port(self.port).credentials(cred).authentication(vec![auth]).build())
  }
}

pub struct MailFormService<S: SumidaState> {
  state:   S,
  session: UserSession,
}

impl<S: SumidaState> MailFormService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  /// 호출자의 인가 주체 집합. 보안 API 키는 키 자신이 곧 정체성이라 소유자의
  /// user/role/organization 으로 펼쳐지지 않는다.
  fn subject(&self) -> SubjectSet {
    SubjectSet::of(&self.session)
  }

  /// 메일 폼은 전역 설정이다 — 만든 사람도 조직도 없으므로 `mail_form#view` 는
  /// `service:mail_form->operate_read` 하나로만 열린다.
  fn select(&self, query: Select<MailForms>) -> Result<Select<MailForms>, SumidaError> {
    Ok(Authz::filter(RESOURCE, "view", &self.subject(), query)?)
  }

  pub fn render(model: &entity::mail_forms::Model, context: impl Serialize) -> Result<String, SumidaError> {
    let mut tt = TinyTemplate::new();
    tt.add_template(&model.kind, &model.content)
      .map_err(|err| SumidaError::MailFormAddTemplateFailed(err.to_string()))?;
    let rendered = tt
      .render(&model.kind, &context)
      .map_err(|err| SumidaError::MailFormRenderFailed(err.to_string()))?;

    Ok(rendered)
  }

  fn _send<R, C>(
    model: &entity::mail_forms::Model,
    receiver: R,
    context: C,
    options: Option<MailFormSendOptions>,
  ) -> Result<(), anyhow::Error>
  where
    R: AsRef<str>,
    C: Serialize,
  {
    let receiver = receiver.as_ref();
    let content = Self::render(model, context)?;

    let email = Message::builder()
      .from(model.sender.parse().map_err(|_err| SumidaError::EmailParseFailed)?)
      .to(receiver.parse().map_err(|_err| SumidaError::EmailParseFailed)?)
      .subject(&model.subject)
      .header(ContentType::TEXT_HTML)
      .body(content)
      .map_err(|err| SumidaError::EmailMessageBuildFailed(err.to_string()))?;

    let mailer = if let Some(opts) = options {
      opts.into_smtp_transport()?
    } else {
      SmtpTransport::unencrypted_localhost()
    };

    match mailer.send(&email) {
      Ok(_) => Ok(()),
      Err(err) => Err(SumidaError::EmailSendFailed(err.to_string()).into()),
    }
  }

  pub async fn send<Kind, Recv, Ctx, Conn>(
    kind: Kind,
    receiver: Recv,
    context: Ctx,
    options: Option<MailFormSendOptions>,
    db: &Conn,
  ) -> Result<(), SumidaError>
  where
    Kind: AsRef<str>,
    Recv: AsRef<str>,
    Ctx: Serialize,
    Conn: ConnectionTrait,
  {
    let kind = kind.as_ref();
    let Some(model) = MailForms::find_by_id(kind).one(db).await? else {
      return Err(SumidaError::NoMailForm(kind.to_owned()));
    };

    Self::_send(&model, receiver, context, options)?;

    Ok(())
  }

  /// The read scope first, the flat parameters inside it, and the condition tree last as one more
  /// conjunct. The sort key resolves through `MailFormFilter` 's own declaration.
  pub async fn list(
    &self,
    filter: MailFormFilter,
    conditions: ConditionQuery,
    pagination: Pagination,
  ) -> Result<(Vec<MailFormListItem>, u64), SumidaError> {
    use entity::mail_forms::Column;

    let txn = self.state.db().begin().await?;

    let mut query = conditions.apply::<MailFormFilter, _>(filter.apply(self.select(MailForms::find())?))?;
    let count = query.clone().select_only().column(Column::Kind).count(&txn).await?;

    query = pagination.order_by_schema::<MailFormFilter, _>(query);
    query = pagination.paginate(query);

    let items = query.into_model::<MailFormListItem>().all(&txn).await?;

    txn.commit().await?;

    Ok((items, count))
  }

  pub async fn get<K>(&self, kind: K) -> Result<Option<GetMailForm>, SumidaError>
  where
    K: AsRef<str>,
  {
    Ok(
      self
        .select(MailForms::find_by_id(kind.as_ref()))?
        .into_model::<GetMailForm>()
        .one(self.state.db())
        .await?,
    )
  }

  pub async fn create(
    &self,
    data: CreateMailForm,
  ) -> Result<InsertResult<entity::mail_forms::ActiveModel>, SumidaError> {
    Ok(MailForms::insert(data.into_active_model()).exec(self.state.db()).await?)
  }

  pub async fn update<K>(
    &self,
    kind: K,
    data: UpdateMailForm,
  ) -> Result<entity::mail_forms::Model, SumidaError>
  where
    K: AsRef<str>,
  {
    let txn = self.state.db().begin().await?;
    Authz::try_check(RESOURCE, kind.as_ref(), "edit", &self.subject(), &txn).await?;

    let mut model = data.into_active_model();
    model.kind = Set(kind.as_ref().to_string());

    let result = MailForms::update(model).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn delete<K>(&self, kind: K) -> Result<DeleteResult, SumidaError>
  where
    K: AsRef<str>,
  {
    let txn = self.state.db().begin().await?;
    Authz::try_check(RESOURCE, kind.as_ref(), "remove", &self.subject(), &txn).await?;

    let result = MailForms::delete_by_id(kind.as_ref()).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }
}
