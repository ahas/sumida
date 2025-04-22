use lettre::message::{header::ContentType, Message};
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::{SmtpTransport, Transport};
use sea_orm::entity::prelude::*;
use serde::Serialize;
use tinytemplate::TinyTemplate;

use crate::core::error::SumidaError;
use crate::core::{db, settings};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "mail_forms")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub kind: String,
  pub sender: String,
  pub subject: String,
  pub content: String,

  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

impl Model {
  pub fn render(&self, context: impl Serialize) -> Result<String, SumidaError> {
    let mut tt = TinyTemplate::new();
    tt.add_template(&self.kind, &self.content)
      .map_err(|err| SumidaError::MailFormAddTemplateFailed(err.to_string()))?;
    let rendered =
      tt.render(&self.kind, &context).map_err(|err| SumidaError::MailFormRenderFailed(err.to_string()))?;

    Ok(rendered)
  }

  pub fn send<R, C>(&self, receiver: R, context: C) -> Result<(), anyhow::Error>
  where
    R: AsRef<str>,
    C: Serialize,
  {
    let receiver = receiver.as_ref();
    let content = self.render(context)?;

    let email = Message::builder()
      .from(self.sender.parse().map_err(|_err| SumidaError::EmailParseFailed)?)
      .to(receiver.parse().map_err(|_err| SumidaError::EmailParseFailed)?)
      .subject(&self.subject)
      .header(ContentType::TEXT_HTML)
      .body(content)
      .map_err(|err| SumidaError::EmailMessageBuildFailed(err.to_string()))?;

    let mailer = if let Some(ref opts) = settings::get().smtp {
      let cred = Credentials::new(opts.username.clone(), opts.password.clone());
      let auth = match opts.auth.to_uppercase().as_str() {
        "PLAIN" => Mechanism::Plain,
        "LOGIN" => Mechanism::Login,
        "XOAUTH2" => Mechanism::Xoauth2,
        _ => Mechanism::Plain,
      };
      SmtpTransport::relay(&opts.server)?.credentials(cred).authentication(vec![auth]).build()
    } else {
      SmtpTransport::unencrypted_localhost()
    };

    match mailer.send(&email) {
      Ok(_) => Ok(()),
      Err(err) => Err(SumidaError::EmailSendFailed(err.to_string()).into()),
    }
  }
}

impl Entity {
  pub async fn send<K, R, C>(kind: K, receiver: R, context: C) -> Result<(), SumidaError>
  where
    K: AsRef<str>,
    R: AsRef<str>,
    C: Serialize,
  {
    let kind = kind.as_ref();
    let db_conn = db::get_connection();
    let Some(form) = Entity::find_by_id(kind).one(db_conn).await? else {
      return Err(SumidaError::NoMailForm(kind.to_owned()));
    };

    form.send(receiver, context)?;

    Ok(())
  }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
