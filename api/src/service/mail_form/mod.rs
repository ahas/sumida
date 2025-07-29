pub mod dto;

use infra::db;
use infra::session::UserContext;
use entity::prelude::*;
use error::SumidaError;
use lettre::message::{Message, header::ContentType};
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::{SmtpTransport, Transport};
use sea_orm::{
  ConnectionTrait, DeleteResult, EntityTrait, InsertResult, IntoActiveModel, PaginatorTrait, QuerySelect,
  Select, Set, TransactionTrait, sea_query::Expr,
};
use serde::Serialize;
use tinytemplate::TinyTemplate;

use crate::service::ApiService;
use crate::util::ap::{Ap, ApGuard};
use crate::util::crud::Pagination;

use self::dto::*;

pub struct MailFormSendOptions {
  pub username: String,
  pub password: String,
  pub auth: String,
  pub server: String,
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

    Ok(SmtpTransport::relay(&self.server)?.credentials(cred).authentication(vec![auth]).build())
  }
}

pub struct MailFormService {
  user_ctx: UserContext,
}

impl MailFormService {
  pub fn new(user_ctx: UserContext) -> Self {
    Self { user_ctx }
  }

  pub fn render(model: &entity::mail_forms::Model, context: impl Serialize) -> Result<String, SumidaError> {
    let mut tt = TinyTemplate::new();
    tt.add_template(&model.kind, &model.content)
      .map_err(|err| SumidaError::MailFormAddTemplateFailed(err.to_string()))?;
    let rendered =
      tt.render(&model.kind, &context).map_err(|err| SumidaError::MailFormRenderFailed(err.to_string()))?;

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

  pub async fn list(
    &self,
    filter: MailFormFilter,
    pagination: Pagination,
  ) -> Result<(Vec<MailFormListItem>, u64), SumidaError> {
    use entity::mail_forms::Column;

    let txn = db::get_connection().begin().await?;

    let mut query = filter.apply(self.select(MailForms::find()));
    let count = query.clone().select_only().column(Column::Kind).count(&txn).await?;

    query = pagination.order_by(query, |col| match col {
      "sender" => Column::Sender,
      "subject" => Column::Subject,
      _ => Column::Kind,
    });
    query = pagination.paginate(query);

    let items = query.into_model::<MailFormListItem>().all(&txn).await?;

    txn.commit().await?;

    Ok((items, count))
  }

  pub async fn get<S>(&self, kind: S) -> Result<Option<GetMailForm>, SumidaError>
  where
    S: AsRef<str>,
  {
    Ok(
      self
        .select(MailForms::find_by_id(kind.as_ref()))
        .into_model::<GetMailForm>()
        .one(db::get_connection())
        .await?,
    )
  }

  pub async fn create(
    &self,
    data: CreateMailForm,
  ) -> Result<InsertResult<entity::mail_forms::ActiveModel>, SumidaError> {
    Ok(MailForms::insert(data.into_active_model()).exec(db::get_connection()).await?)
  }

  pub async fn update<S>(
    &self,
    kind: S,
    data: UpdateMailForm,
  ) -> Result<entity::mail_forms::Model, SumidaError>
  where
    S: AsRef<str>,
  {
    let txn = db::get_connection().begin().await?;
    self.ap().action("update").check(MailForms::find_by_id(kind.as_ref()), &txn).await?;

    let mut model = data.into_active_model();
    model.kind = Set(kind.as_ref().to_string());

    let result = MailForms::update(model).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn delete<S>(&self, kind: S) -> Result<DeleteResult, SumidaError>
  where
    S: AsRef<str>,
  {
    let txn = db::get_connection().begin().await?;
    self.ap().action("delete").check(MailForms::find_by_id(kind.as_ref()), &txn).await?;

    let result = MailForms::delete_by_id(kind.as_ref()).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }
}

impl ApiService<Ap, MailForms> for MailFormService {
  fn select(&self, query: Select<MailForms>) -> Select<MailForms> {
    self.ap().action("read").apply(query)
  }

  fn ap(&self) -> Ap {
    use entity::mail_forms::Column;

    Ap::new("mail_form") //
      .owner(&self.user_ctx)
      .object(Expr::col((MailForms, Column::Kind)).cast_as("varchar"))
  }
}
