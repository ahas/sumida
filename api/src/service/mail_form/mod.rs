pub mod dto;

use core::db;
use entity::prelude::*;
use error::SumidaError;
use lettre::message::{Message, header::ContentType};
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::{SmtpTransport, Transport};
use sea_orm::{
  ConnectionTrait, DeleteResult, EntityTrait, InsertResult, IntoActiveModel, PaginatorTrait, QuerySelect,
  Set, TransactionTrait,
};
use serde::Serialize;
use tinytemplate::TinyTemplate;

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

pub struct MailFormService;

impl MailFormService {
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
    filter: MailFormFilter,
    pagination: Pagination,
  ) -> Result<(Vec<MailFormListItem>, u64), SumidaError> {
    use entity::mail_forms::Column;

    let db_conn = db::get_connection();
    let txn = db_conn.begin().await?;

    let mut query = filter.apply(MailForms::find());
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

  pub async fn get<S>(kind: S) -> Result<Option<GetMailForm>, SumidaError>
  where
    S: AsRef<str>,
  {
    let db_conn = db::get_connection();
    Ok(MailForms::find_by_id(kind.as_ref()).into_model::<GetMailForm>().one(db_conn).await?)
  }

  pub async fn create(
    data: CreateMailForm,
  ) -> Result<InsertResult<entity::mail_forms::ActiveModel>, SumidaError> {
    let db_conn = db::get_connection();
    Ok(MailForms::insert(data.into_active_model()).exec(db_conn).await?)
  }

  pub async fn update<S>(kind: S, data: UpdateMailForm) -> Result<entity::mail_forms::Model, SumidaError>
  where
    S: AsRef<str>,
  {
    let mut model = data.into_active_model();
    model.kind = Set(kind.as_ref().to_string());

    let db_conn = db::get_connection();
    Ok(MailForms::update(model).exec(db_conn).await?)
  }

  pub async fn delete<S>(kind: S) -> Result<DeleteResult, SumidaError>
  where
    S: AsRef<str>,
  {
    let db_conn = db::get_connection();
    Ok(MailForms::delete_by_id(kind.as_ref()).exec(db_conn).await?)
  }
}
