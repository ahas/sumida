use api::service::prelude::*;
use e2e::common::prelude::*;
use entity::prelude::*;
use infra::db;
use reqwest::StatusCode;
use sea_orm::{EntityTrait, Set};

#[tokio::test]
async fn user_signup() {
  let server = use_server().await;
  let db = db::get_connection();
  let user = TestUser::create(db).await.expect("Failed to create a test user");
  println!("User password = {}", user.password);

  AccessPolicies::insert(entity::access_policies::ActiveModel {
    namespace: Set("*".to_string()),
    user: Set("user:admin".to_string()),
    object: Set("*".to_string()),
    action: Set(vec!["*".to_string()]),
    effect: Set("allow".to_string()),
    ..Default::default()
  })
  .exec(db)
  .await
  .expect("Failed to insert the admin access policy");

  let path = "/auth/sessions";
  let resp = server
    .post(path)
    .json(&LoginSession {
      username: user.model.username.clone(),
      password: user.password.clone(),
      remember_me: Some(false),
    })
    .send()
    .await
    .expect(&format!("Failed to send a POST request to {path}"));
  let status_code = resp.status();
  let json = resp.json::<serde_json::Value>().await.expect("Failed to parse the user item into JSON");

  println!("Request = {path}");
  println!("JSON value = {json}");
  assert_eq!(status_code, StatusCode::OK);

  let path = &format!("/users/{}", user.model.id);
  let resp = server.get(path).send().await.expect(&format!("Failed to fetch from {path}"));
  let status_code = resp.status();
  let json = resp.json::<serde_json::Value>().await.expect("Failed to parse the user item into JSON");

  println!("Request = {path}");
  println!("JSON value = {}", json);
  assert_eq!(status_code, StatusCode::OK);

  let item: GetUser = serde_json::from_value(json).expect("Failed to parse the json into GetUser");
  assert_eq!(user.model.id, item.id);
}
