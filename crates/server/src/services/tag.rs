//! Tag service — ports `service/tag.go`.

use sea_orm::*;

use ::entity::tag;

use crate::services::{now, paginate};

pub async fn info_by_id(db: &DatabaseConnection, id: i32) -> Result<Option<tag::Model>, DbErr> {
    tag::Entity::find_by_id(id).one(db).await
}

pub async fn info_by_user_name_collection(
    db: &DatabaseConnection,
    user_id: i32,
    name: &str,
    collection_id: i32,
) -> Result<Option<tag::Model>, DbErr> {
    tag::Entity::find()
        .filter(tag::Column::UserId.eq(user_id))
        .filter(tag::Column::Name.eq(name))
        .filter(tag::Column::CollectionId.eq(collection_id))
        .one(db)
        .await
}

pub async fn list_by_user_and_collection(
    db: &DatabaseConnection,
    user_id: i32,
    collection_id: i32,
) -> Result<Vec<tag::Model>, DbErr> {
    tag::Entity::find()
        .filter(tag::Column::UserId.eq(user_id))
        .filter(tag::Column::CollectionId.eq(collection_id))
        .order_by_asc(tag::Column::Name)
        .all(db)
        .await
}

pub async fn list_by_user_id(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<Vec<tag::Model>, DbErr> {
    tag::Entity::find()
        .filter(tag::Column::UserId.eq(user_id))
        .all(db)
        .await
}

pub struct TagListResult {
    pub list: Vec<tag::Model>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

pub async fn list(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
) -> Result<TagListResult, DbErr> {
    list_filtered(db, page, page_size, None, None).await
}

pub async fn list_filtered(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
    user_id: Option<i32>,
    collection_id: Option<i32>,
) -> Result<TagListResult, DbErr> {
    let (page, page_size) = paginate(page, page_size);
    let mut q = tag::Entity::find();
    if let Some(uid) = user_id.filter(|v| *v > 0) {
        q = q.filter(tag::Column::UserId.eq(uid));
    }
    if let Some(cid) = collection_id.filter(|v| *v >= 0) {
        q = q.filter(tag::Column::CollectionId.eq(cid));
    }
    let total = q.clone().count(db).await? as i64;
    let list = q
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;
    Ok(TagListResult {
        list,
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

pub async fn create(
    db: &DatabaseConnection,
    name: &str,
    color: i64,
    user_id: i32,
    collection_id: i32,
) -> Result<tag::Model, DbErr> {
    let am = tag::ActiveModel {
        name: Set(name.to_string()),
        color: Set(color),
        user_id: Set(user_id),
        collection_id: Set(collection_id),
        created_at: Set(now()),
        updated_at: Set(now()),
        ..Default::default()
    };
    am.insert(db).await
}

pub async fn update(db: &DatabaseConnection, model: &tag::Model) -> Result<(), DbErr> {
    let am = tag::ActiveModel {
        id: Set(model.id),
        name: Set(model.name.clone()),
        color: Set(model.color),
        user_id: Set(model.user_id),
        collection_id: Set(model.collection_id),
        updated_at: Set(now()),
        ..Default::default()
    };
    am.update(db).await?;
    Ok(())
}

pub async fn delete(db: &DatabaseConnection, id: i32) -> Result<(), DbErr> {
    tag::Entity::delete_by_id(id).exec(db).await?;
    Ok(())
}

/// Reconcile a user's tags against `name -> color` (≈ `TagService.UpdateTags`).
pub async fn update_tags(
    db: &DatabaseConnection,
    user_id: i32,
    mut tags: std::collections::HashMap<String, i64>,
) -> Result<(), DbErr> {
    let existing = list_by_user_id(db, user_id).await?;
    for t in existing {
        match tags.get(&t.name).copied() {
            None => {
                tag::Entity::delete_by_id(t.id).exec(db).await?;
            }
            Some(color) => {
                if color != t.color {
                    let mut am: tag::ActiveModel = t.clone().into();
                    am.color = Set(color);
                    am.updated_at = Set(now());
                    am.update(db).await?;
                }
                tags.remove(&t.name);
            }
        }
    }
    for (name, color) in tags {
        create(db, &name, color, user_id, 0).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectionTrait, Database, DbBackend, Schema};

    #[tokio::test]
    async fn update_persists_color_changes() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let schema = Schema::new(DbBackend::Sqlite);
        db.execute(
            db.get_database_backend()
                .build(&schema.create_table_from_entity(tag::Entity)),
        )
        .await
        .unwrap();

        let mut tag = create(&db, "red", 0xFFFF0000, 1, 2).await.unwrap();
        tag.color = 0xFF00FF00;
        update(&db, &tag).await.unwrap();

        let updated = info_by_id(&db, tag.id).await.unwrap().unwrap();
        assert_eq!(updated.color, 0xFF00FF00);
    }
}
