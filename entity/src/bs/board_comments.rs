use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "board_comments")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub id: i32,
  pub post_id: i32,
  pub user_id: i32,
  pub parent_id: Option<i32>,
  pub content: Option<String>,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
  #[sea_orm(
    belongs_to = "super::board_posts::Entity",
    from = "Column::PostId",
    to = "super::board_posts::Column::Id",
    on_delete = "Cascade"
  )]
  Post,
  #[sea_orm(belongs_to = "Entity", from = "Column::ParentId", to = "Column::Id", on_delete = "Cascade")]
  Parent,
  #[sea_orm(belongs_to = "crate::users::Entity", from = "Column::UserId", to = "crate::users::Column::Id")]
  User,
}

impl Related<super::board_posts::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::Post.def()
  }
}

impl Related<Entity> for Entity {
  fn to() -> RelationDef {
    Relation::Parent.def()
  }
}

impl Related<crate::users::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::User.def()
  }
}

impl ActiveModelBehavior for ActiveModel {}
