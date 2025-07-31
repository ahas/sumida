pub mod dto;

use crate::util::pagination::Pagination;
use infra::db;
use infra::session::UserSession;
use entity::prelude::*;
use error::SumidaError;
use sea_orm::ActiveValue::Set;
use sea_orm::sea_query::Alias;
use sea_orm::sea_query::{Asterisk, Expr};
use sea_orm::{
  DeleteResult, EntityTrait, InsertResult, IntoActiveModel, IntoSimpleExpr, JoinType, PaginatorTrait,
  QuerySelect, RelationTrait, Select, TransactionTrait,
};

use self::dto::*;

pub struct BoardPostService;

impl BoardPostService {
  pub fn select(query: Select<BoardPosts>) -> Select<BoardPosts> {
    let views_alias = Alias::new("views");
    query
      .select_only()
      .expr(Expr::col((BoardPosts, Asterisk)))
      .expr(Expr::count(Expr::col((views_alias.clone(), "id"))))
      .join_as(JoinType::LeftJoin, entity::bs::board_posts::Relation::Views.def(), views_alias.clone())
  }

  pub async fn list(
    filter: BoardPostFilter,
    pagination: Pagination,
  ) -> Result<(Vec<BoardPostListItem>, u64), SumidaError> {
    use entity::bs::board_posts::Column;

    let db = db::get_connection();
    let txn = db.begin().await?;

    let mut query = filter.apply(Self::select(BoardPosts::find()));

    let count = query.clone().select_only().column(Column::Id).count(&txn).await?;

    query = pagination.order_by(query, |col| match col {
      "title" => Column::Title.into_simple_expr(),
      _ => Column::CreatedAt.into_simple_expr(),
    });
    query = pagination.paginate(query);

    let items = query.into_partial_model::<BoardPostListItem>().all(&txn).await?;

    txn.commit().await?;
    Ok((items, count))
  }

  pub async fn get(id: i32) -> Result<Option<GetBoardPost>, SumidaError> {
    Ok(
      Self::select(BoardPosts::find_by_id(id))
        .into_partial_model::<GetBoardPost>()
        .one(db::get_connection())
        .await?,
    )
  }

  pub async fn view(session: &UserSession, id: i32) -> Result<Option<GetBoardPost>, SumidaError> {
    let txn = db::get_connection().begin().await?;
    let post =
      Self::select(BoardPosts::find_by_id(id)).into_partial_model::<GetBoardPost>().one(&txn).await?;

    let mut view_am = entity::bs::board_post_views::ActiveModel {
      post_id: Set(id),
      ..Default::default()
    };

    if session.profile.is_authorized {
      view_am.user_id = Set(Some(session.profile.id));
    } else {
      view_am.ip_address = Set(Some(session.env.ip_address.clone()));
    }

    BoardPostViews::insert(view_am).exec(&txn).await?;

    txn.commit().await?;

    Ok(post)
  }

  pub async fn create(
    data: CreateBoardPost,
  ) -> Result<InsertResult<entity::bs::board_posts::ActiveModel>, SumidaError> {
    Ok(BoardPosts::insert(data.into_active_model()).exec(db::get_connection()).await?)
  }

  pub async fn update(
    id: i32,
    data: UpdateBoardPost,
  ) -> Result<entity::bs::board_posts::Model, SumidaError> {
    let mut am = data.into_active_model();
    am.id = Set(id);
    Ok(BoardPosts::update(am).exec(db::get_connection()).await?)
  }

  pub async fn delete(id: i32) -> Result<DeleteResult, SumidaError> {
    Ok(BoardPosts::delete_by_id(id).exec(db::get_connection()).await?)
  }
}
