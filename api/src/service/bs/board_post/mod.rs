pub mod dto;

use crate::SumidaState;
use crate::util::pagination::Pagination;
use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::ActiveValue::Set;
use sea_orm::sea_query::Alias;
use sea_orm::sea_query::{Asterisk, Expr};
use sea_orm::{
  DeleteResult, EntityTrait, InsertResult, IntoActiveModel, IntoSimpleExpr, JoinType, PaginatorTrait,
  QuerySelect, RelationTrait, Select, TransactionTrait,
};

use self::dto::*;

pub struct BoardPostService<S: SumidaState> {
  state: S,
  session: UserSession,
}

impl<S: SumidaState> BoardPostService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  pub fn select(&self, query: Select<BoardPosts>) -> Select<BoardPosts> {
    let views_alias = Alias::new("views");
    query
      .select_only()
      .expr(Expr::col((BoardPosts, Asterisk)))
      .expr(Expr::count(Expr::col((views_alias.clone(), "id"))))
      .join_as(JoinType::LeftJoin, entity::bs::board_posts::Relation::Views.def(), views_alias.clone())
  }

  pub async fn list(
    &self,
    filter: BoardPostFilter,
    pagination: Pagination,
  ) -> Result<(Vec<BoardPostListItem>, u64), SumidaError> {
    use entity::bs::board_posts::Column;

    let txn = self.state.db().begin().await?;

    let mut query = filter.apply(self.select(BoardPosts::find()));

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

  pub async fn get(&self, id: i32) -> Result<Option<GetBoardPost>, SumidaError> {
    Ok(
      self
        .select(BoardPosts::find_by_id(id))
        .into_partial_model::<GetBoardPost>()
        .one(self.state.db())
        .await?,
    )
  }

  pub async fn view(&self, session: &UserSession, id: i32) -> Result<Option<GetBoardPost>, SumidaError> {
    let txn = self.state.db().begin().await?;
    let post =
      self.select(BoardPosts::find_by_id(id)).into_partial_model::<GetBoardPost>().one(&txn).await?;

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
    &self,
    data: CreateBoardPost,
  ) -> Result<InsertResult<entity::bs::board_posts::ActiveModel>, SumidaError> {
    Ok(BoardPosts::insert(data.into_active_model()).exec(self.state.db()).await?)
  }

  pub async fn update(
    &self,
    id: i32,
    data: UpdateBoardPost,
  ) -> Result<entity::bs::board_posts::Model, SumidaError> {
    let mut am = data.into_active_model();
    am.id = Set(id);
    Ok(BoardPosts::update(am).exec(self.state.db()).await?)
  }

  pub async fn delete(&self, id: i32) -> Result<DeleteResult, SumidaError> {
    Ok(BoardPosts::delete_by_id(id).exec(self.state.db()).await?)
  }
}
