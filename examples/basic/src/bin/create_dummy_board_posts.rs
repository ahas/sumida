use chrono::Utc;
use entity::bps::board_posts::{BoardPostKind, BoardPostStatus};
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

  println!("더미 데이터 생성 시작...");

  // 1. 조직 생성
  let org = organizations::ActiveModel {
    name: Set("테스트 조직".to_string()),
    description: Set(Some("더미 데이터용 테스트 조직".to_string())),
    is_enabled: Set(true),
    created_at: Set(now),
    updated_at: Set(now),
    ..Default::default()
  };
  let org = org.insert(&db).await?;
  println!("조직 생성됨: {}", org.name);

  // 2. 역할 생성
  let role = roles::ActiveModel {
    name: Set("사용자".to_string()),
    description: Set(Some("일반 사용자 역할".to_string())),
    level: Set(1),
    is_enabled: Set(true),
    created_at: Set(now),
    updated_at: Set(now),
    ..Default::default()
  };
  let role = role.insert(&db).await?;
  println!("역할 생성됨: {}", role.name);

  // 3. 사용자들 생성
  let mut users_data = Vec::new();
  for i in 1..=5 {
    let user = users::ActiveModel {
      username: Set(format!("user{}", i)),
      password: Set("hashed_password".to_string()),
      email: Set(Some(format!("user{}@example.com", i))),
      full_name: Set(Some(format!("사용자 {}", i))),
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
    println!("사용자 생성됨: {}", users_data.last().unwrap().username);
  }

  // 4. 게시판 그룹 생성
  let board_groups_data = vec![
    ("자유게시판", "자유롭게 이야기하는 공간"),
    ("질문게시판", "궁금한 것들을 질문하는 공간"),
    ("공지사항", "중요한 공지사항을 올리는 공간"),
  ];

  let mut groups = Vec::new();
  for (i, (name, desc)) in board_groups_data.iter().enumerate() {
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
    println!("게시판 그룹 생성됨: {}", name);
  }

  // 5. 게시판 생성
  let boards_data = vec![
    (0, "자유 토론", "자유롭게 이야기해요", true, 10485760, 5), // 10MB
    (1, "개발 질문", "개발 관련 질문", true, 5242880, 3),       // 5MB
    (1, "일반 질문", "일반적인 질문", false, 2097152, 2),       // 2MB
    (2, "서비스 공지", "서비스 관련 공지", false, 0, 0),
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
    println!("게시판 생성됨: {}", name);
  }

  // 6. 게시판 토픽 생성
  let topics_data = vec![
    (0, "일반", "일반적인 주제"),
    (0, "유머", "재미있는 이야기"),
    (1, "Backend", "백엔드 개발 질문"),
    (1, "Frontend", "프론트엔드 개발 질문"),
    (2, "생활", "일상생활 관련 질문"),
    (3, "업데이트", "서비스 업데이트 공지"),
    (3, "점검", "서비스 점검 공지"),
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
    println!("토픽 생성됨: {}", name);
  }

  // 7. 게시글 생성
  let posts_data = vec![
    // 자유게시판 - 일반 토픽
    (
      0,
      0,
      0,
      BoardPostKind::Article,
      "안녕하세요!",
      "첫 번째 게시글입니다. 반갑습니다!",
      vec!["인사", "첫글"],
    ),
    (
      0,
      0,
      1,
      BoardPostKind::Article,
      "Rust는 정말 좋은 언어네요",
      "메모리 안전성도 보장하고 성능도 좋고...",
      vec!["rust", "개발"],
    ),
    // 자유게시판 - 유머 토픽
    (
      0,
      1,
      2,
      BoardPostKind::Article,
      "개발자 유머",
      "컴파일러와 싸우다가 이겼습니다",
      vec!["유머", "개발"],
    ),
    // 개발 질문 - Backend 토픽
    (
      1,
      2,
      0,
      BoardPostKind::Article,
      "Rust에서 async/await 질문",
      "async 함수를 어떻게 효율적으로 사용하나요?",
      vec!["rust", "async", "질문"],
    ),
    (
      1,
      2,
      1,
      BoardPostKind::Article,
      "DB 연결 풀 관리",
      "SeaORM에서 커넥션 풀은 어떻게 관리하는게 좋을까요?",
      vec!["database", "seaorm"],
    ),
    // 개발 질문 - Frontend 토픽
    (
      1,
      3,
      3,
      BoardPostKind::Article,
      "Vue.js 상태 관리",
      "Pinia vs Vuex 어떤 걸 쓰는게 좋을까요?",
      vec!["vue", "상태관리"],
    ),
    // 일반 질문 - 생활 토픽
    (
      2,
      4,
      4,
      BoardPostKind::Article,
      "맛있는 커피 추천",
      "개발하면서 마실 커피 추천해주세요",
      vec!["커피", "추천"],
    ),
    // 공지사항 - 업데이트 토픽
    (
      3,
      5,
      1,
      BoardPostKind::Article,
      "서비스 업데이트 v1.2.0",
      "새로운 기능들이 추가되었습니다.",
      vec!["공지", "업데이트"],
    ),
    // 공지사항 - 점검 토픽
    (
      3,
      6,
      1,
      BoardPostKind::Article,
      "정기 점검 안내",
      "매주 일요일 새벽 2시-4시 정기점검이 있습니다.",
      vec!["공지", "점검"],
    ),
  ];

  for (board_idx, topic_idx, user_idx, kind, title, content, tags) in posts_data {
    let board_id = boards_list[board_idx].id;
    let topic_id = topics_list[topic_idx].id;
    let user_id = users_data[user_idx].id;

    let post = board_posts::ActiveModel {
      board_id: Set(board_id),
      topic_id: Set(topic_id),
      parent_id: Set(None),
      user_id: Set(Some(user_id)),
      kind: Set(kind),
      status: Set(BoardPostStatus::Published),
      revision: Set(Some(1)),
      title: Set(title.to_string()),
      compression_type: Set(None),
      content_type: Set(Some("text/html".to_string())),
      content: Set(Some(plain_text_to_html(content))),
      text: Set(Some(content.to_string())),
      tags: Set(Some(tags.iter().map(|s| s.to_string()).collect())),
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
    println!("게시글 생성됨: {} (ID: {})", title, post.id);
  }

  // 8. 댓글 생성
  let comments_data = vec![
    (0, 0, "좋은 글이네요! 감사합니다."),
    (0, 1, "저도 동감합니다."),
    (3, 2, "async/await는 정말 편리해요. tokio 문서를 참고해보세요."),
    (4, 3, "답변 감사합니다!"),
    (6, 4, "아메리카노 추천합니다!"),
  ];

  // 게시글 ID들을 다시 조회
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
        content_type: Set(Some("text/html".to_string())),
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
      println!("댓글 생성됨: {} (Parent: {})", comment.id, parent_post.id);
    }
  }

  println!("더미 데이터 생성 완료!");
  println!("생성된 데이터:");
  println!("- 조직: 1개");
  println!("- 역할: 1개");
  println!("- 사용자: {}명", users_data.len());
  println!("- 게시판 그룹: {}개", groups.len());
  println!("- 게시판: {}개", boards_list.len());
  println!("- 토픽: {}개", topics_list.len());
  println!("- 게시글: {}개 (댓글 포함)", published_posts.len() + comments_data.len());

  Ok(())
}
