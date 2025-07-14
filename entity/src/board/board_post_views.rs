// entity/board_post_views.rs
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "board_post_views")]
pub struct Model {
  #[sea_orm(primary_key, auto_increment = true)]
  pub id: i32,
  pub post_id: i32,
  pub user_id: Option<i32>,
  pub ip_address: Option<String>,
  pub viewed_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
  #[sea_orm(
    belongs_to = "super::board_posts::Entity",
    from = "Column::PostId",
    to = "super::board_posts::Column::Id",
    on_delete = "Cascade",
    on_update = "Cascade"
  )]
  BoardPost,
}

impl Related<super::board_posts::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::BoardPost.def()
  }
}

impl ActiveModelBehavior for ActiveModel {}
