use entity::prelude::*;
use sea_orm::{QuerySelect, Select};

pub fn with_file_columns(query: Select<BoardFiles>) -> Select<BoardFiles> {
  query
    .column_as(FilesColumn::Dir, "dir")
    .column_as(FilesColumn::Name, "name")
    .column_as(FilesColumn::Uri, "uri")
    .column_as(FilesColumn::Size, "size")
    .column_as(FilesColumn::MimeType, "mime_type")
    .column_as(FilesColumn::Status, "status")
    .column_as(FilesColumn::ExpiresAt, "expires_at")
    .column_as(FilesColumn::CreatedAt, "created_at")
    .column_as(FilesColumn::UpdatedAt, "updated_at")
    .column_as(FilesColumn::UpdatedAt, "deleted_at")
}
