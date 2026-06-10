use chrono::Utc;
use entity::bps::board_posts::{BoardPostContentType, BoardPostKind, BoardPostStatus};
use entity::bps::boards::BoardPostRecordVisibility;
use entity::bps::{board_groups, board_posts, board_topics, boards};
use entity::{organizations, roles, users};
use sea_orm::prelude::*;
use sea_orm::{ActiveModelTrait, Set};
use serde_json::json;
use sumida::infra;

fn plain_text_to_html(text: &str) -> String {
  let trimmed = text.trim();
  if trimmed.is_empty() {
    return "<p></p>".to_string();
  }

  trimmed
    .split("\n\n")
    .map(|section| {
      let body = section.trim().replace('\n', "<br />");
      format!("<p>{}</p>", body)
    })
    .collect::<Vec<_>>()
    .join("")
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  let db = infra::db::connect().await;
  let now = Utc::now().fixed_offset();

  println!("Starting dummy data creation...");

  // 1. Create organization
  let org = organizations::ActiveModel {
    name: Set("Test Organization".to_string()),
    created_at: Set(now),
    updated_at: Set(now),
    ..Default::default()
  };
  let org = org.insert(&db).await?;
  println!("Organization created: {}", org.name);

  // 2. Create role
  let role = roles::ActiveModel {
    name: Set("User".to_string()),
    description: Set(Some("Regular user role".to_string())),
    created_at: Set(now),
    updated_at: Set(now),
    ..Default::default()
  };
  let role = role.insert(&db).await?;
  println!("Role created: {}", role.name);

  // 3. Create users
  let mut users_data = Vec::new();
  for i in 1..=5 {
    let user = users::ActiveModel {
      username: Set(format!("user{}", i)),
      password: Set("hashed_password".to_string()),
      email: Set(Some(format!("user{}@example.com", i))),
      full_name: Set(Some(format!("User {}", i))),
      is_admin: Set(false),
      is_enabled: Set(true),
      role_id: Set(role.id),
      organization_id: Set(org.id),
      created_at: Set(now),
      updated_at: Set(now),
      ..Default::default()
    };
    let user = user.insert(&db).await?;
    users_data.push(user);
    println!("User created: {}", users_data.last().unwrap().username);
  }

  // 4. Create board groups
  let board_groups_data = vec![
    ("Free Board", "A space for free discussion"),
    ("Q&A Board", "A space to ask questions"),
    ("Notices", "A space for important announcements"),
  ];

  let mut groups = Vec::new();
  for (i, (name, desc)) in board_groups_data.into_iter().enumerate() {
    let group = board_groups::ActiveModel {
      parent_id: Set(None),
      name: Set(name.to_string()),
      description: Set(Some(desc.to_string())),
      order: Set(i as i32),
      created_at: Set(now),
      updated_at: Set(now),
      ..Default::default()
    };
    let group = group.insert(&db).await?;
    groups.push(group);
    println!("Board group created: {}", name);
  }

  // 5. Create boards
  let boards_data = vec![
    (0, "Free Talk", "Talk about anything", true, 10485760, 5), // 10MB
    (1, "Dev Questions", "Development-related questions", true, 5242880, 3), // 5MB
    (1, "General Questions", "General questions", false, 2097152, 2), // 2MB
    (2, "Service Notices", "Service-related notices", false, 0, 0),
  ];

  let mut boards_list = Vec::new();
  for (group_idx, name, desc, can_upload, max_size, max_count) in boards_data {
    let board = boards::ActiveModel {
      group_id: Set(Some(groups[group_idx].id)),
      name: Set(name.to_string()),
      description: Set(Some(desc.to_string())),
      order: Set(boards_list.len() as i32),
      can_upload_file: Set(can_upload),
      max_file_size: Set(max_size),
      max_file_count: Set(max_count),
      allowed_file_types: Set(Some(json!(["jpg", "jpeg", "png", "gif", "pdf", "txt"]))),
      reaction_visibility: Set(BoardPostRecordVisibility::Public),
      viewer_visibility: Set(BoardPostRecordVisibility::Public),
      is_versioned: Set(true),
      is_enabled: Set(true),
      created_at: Set(now),
      updated_at: Set(now),
      ..Default::default()
    };
    let board = board.insert(&db).await?;
    boards_list.push(board);
    println!("Board created: {}", name);
  }

  // 6. Create board topics
  let topics_data = vec![
    (0, "General", "General topics"),
    (0, "Humor", "Fun stories"),
    (1, "Backend", "Backend development questions"),
    (1, "Frontend", "Frontend development questions"),
    (2, "Life", "Questions about everyday life"),
    (3, "Updates", "Service update notices"),
    (3, "Maintenance", "Service maintenance notices"),
  ];

  let mut topics_list = Vec::new();
  for (board_idx, name, desc) in topics_data {
    let topic = board_topics::ActiveModel {
      board_id: Set(boards_list[board_idx].id),
      name: Set(name.to_string()),
      description: Set(Some(desc.to_string())),
      order: Set(topics_list.len() as i32),
      created_at: Set(now),
      updated_at: Set(now),
      ..Default::default()
    };
    let topic = topic.insert(&db).await?;
    topics_list.push(topic);
    println!("Topic created: {}", name);
  }

  // 7. Create posts
  let posts_data = vec![
    // Free board - General topic
    (
      0,
      0,
      0,
      BoardPostKind::Article,
      "Hello!",
      "This is the first post. Nice to meet you!",
      vec!["greeting", "first-post"],
    ),
    (
      0,
      0,
      1,
      BoardPostKind::Article,
      "Rust is a really nice language",
      "It guarantees memory safety and has great performance...",
      vec!["rust", "dev"],
    ),
    // Free board - Humor topic
    (
      0,
      1,
      2,
      BoardPostKind::Article,
      "Developer humor",
      "I fought the compiler and won",
      vec!["humor", "dev"],
    ),
    // Dev questions - Backend topic
    (
      1,
      2,
      0,
      BoardPostKind::Article,
      "Question about async/await in Rust",
      "How do I use async functions efficiently?",
      vec!["rust", "async", "question"],
    ),
    (
      1,
      2,
      1,
      BoardPostKind::Article,
      "Managing the DB connection pool",
      "What's the best way to manage the connection pool in SeaORM?",
      vec!["database", "seaorm"],
    ),
    // Dev questions - Frontend topic
    (
      1,
      3,
      3,
      BoardPostKind::Article,
      "Vue.js state management",
      "Pinia vs Vuex — which one should I use?",
      vec!["vue", "state-management"],
    ),
    // General questions - Life topic
    (
      2,
      4,
      4,
      BoardPostKind::Article,
      "Recommend a good coffee",
      "Please recommend coffee to drink while coding",
      vec!["coffee", "recommendation"],
    ),
    // Notices - Update topic
    (
      3,
      5,
      1,
      BoardPostKind::Article,
      "Service update v1.2.0",
      "New features have been added.",
      vec!["notice", "update"],
    ),
    // Notices - Maintenance topic
    (
      3,
      6,
      1,
      BoardPostKind::Article,
      "Scheduled maintenance notice",
      "There is scheduled maintenance every Sunday from 2 AM to 4 AM.",
      vec!["notice", "maintenance"],
    ),
  ];

  for (board_idx, topic_idx, user_idx, kind, title, content, tags) in posts_data {
    let board_id = boards_list[board_idx].id;
    let topic_id = topics_list[topic_idx].id;
    let user_id = users_data[user_idx].id;

    let post = board_posts::ActiveModel {
      board_id: Set(board_id),
      topic_id: Set(Some(topic_id)),
      parent_id: Set(None),
      user_id: Set(Some(user_id)),
      kind: Set(kind),
      status: Set(BoardPostStatus::Published),
      revision: Set(Some(1)),
      title: Set(title.to_string()),
      compression_type: Set(None),
      content_type: Set(BoardPostContentType::Html),
      content: Set(Some(plain_text_to_html(content))),
      text: Set(Some(content.to_string())),
      tags: Set(Some(tags.into_iter().map(|s| s.to_string()).collect())),
      views: Set(0),
      reactions: Set(Some(json!({}))),
      published_at: Set(Some(now)),
      scheduled_at: Set(None),
      created_at: Set(now),
      updated_at: Set(now),
      deleted_at: Set(None),
      ..Default::default()
    };

    let post = post.insert(&db).await?;
    println!("Post created: {} (ID: {})", title, post.id);
  }

  // 8. Create comments
  let comments_data = vec![
    (0, 0, "Great post! Thank you."),
    (0, 1, "I agree."),
    (3, 2, "async/await is really convenient. Check out the tokio docs."),
    (4, 3, "Thanks for the answer!"),
    (6, 4, "I recommend an americano!"),
  ];
  let comments_count = comments_data.len();

  // Re-query the post IDs
  let published_posts: Vec<board_posts::Model> = board_posts::Entity::find()
    .filter(board_posts::Column::Status.eq(BoardPostStatus::Published))
    .all(&db)
    .await?;

  for (post_idx, user_idx, comment_content) in comments_data {
    if let Some(parent_post) = published_posts.get(post_idx) {
      let comment = board_posts::ActiveModel {
        board_id: Set(parent_post.board_id),
        topic_id: Set(parent_post.topic_id),
        parent_id: Set(Some(parent_post.id)),
        user_id: Set(Some(users_data[user_idx].id)),
        kind: Set(BoardPostKind::Comment),
        status: Set(BoardPostStatus::Published),
        revision: Set(Some(1)),
        title: Set("".to_string()),
        compression_type: Set(None),
        content_type: Set(BoardPostContentType::Html),
        content: Set(Some(plain_text_to_html(comment_content))),
        text: Set(Some(comment_content.to_string())),
        tags: Set(None),
        views: Set(0),
        reactions: Set(Some(json!({}))),
        published_at: Set(Some(now)),
        scheduled_at: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
        deleted_at: Set(None),
        ..Default::default()
      };

      let comment = comment.insert(&db).await?;
      println!("Comment created: {} (Parent: {})", comment.id, parent_post.id);
    }
  }

  println!("Dummy data creation complete!");
  println!("Created data:");
  println!("- Organizations: 1");
  println!("- Roles: 1");
  println!("- Users: {}", users_data.len());
  println!("- Board groups: {}", groups.len());
  println!("- Boards: {}", boards_list.len());
  println!("- Topics: {}", topics_list.len());
  println!("- Posts: {} (including comments)", published_posts.len() + comments_count);

  Ok(())
}
