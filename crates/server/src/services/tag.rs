//! Tag service — ports `service/tag.go`.

use std::collections::HashSet;

use sea_orm::*;

use ::entity::{address_book, tag};

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

pub async fn rename_with_peers(
    db: &DatabaseConnection,
    model: &tag::Model,
    new_name: &str,
) -> Result<(), DbErr> {
    let transaction = db.begin().await?;
    let current = tag::Entity::find_by_id(model.id)
        .filter(tag::Column::UserId.eq(model.user_id))
        .filter(tag::Column::CollectionId.eq(model.collection_id))
        .one(&transaction)
        .await?
        .ok_or_else(|| DbErr::RecordNotFound("Tag not found".to_string()))?;
    tag::ActiveModel {
        id: Set(current.id),
        name: Set(new_name.to_string()),
        updated_at: Set(now()),
        ..Default::default()
    }
    .update(&transaction)
    .await?;
    update_peer_tags(&transaction, &current, Some(new_name)).await?;
    transaction.commit().await
}

pub async fn delete_with_peers(db: &DatabaseConnection, model: &tag::Model) -> Result<(), DbErr> {
    let transaction = db.begin().await?;
    let current = tag::Entity::find_by_id(model.id)
        .filter(tag::Column::UserId.eq(model.user_id))
        .filter(tag::Column::CollectionId.eq(model.collection_id))
        .one(&transaction)
        .await?
        .ok_or_else(|| DbErr::RecordNotFound("Tag not found".to_string()))?;
    tag::Entity::delete_by_id(current.id)
        .exec(&transaction)
        .await?;
    update_peer_tags(&transaction, &current, None).await?;
    transaction.commit().await
}

async fn update_peer_tags(
    transaction: &DatabaseTransaction,
    model: &tag::Model,
    replacement: Option<&str>,
) -> Result<(), DbErr> {
    let peers = address_book::Entity::find()
        .filter(address_book::Column::UserId.eq(model.user_id))
        .filter(address_book::Column::CollectionId.eq(model.collection_id))
        .all(transaction)
        .await?;
    for peer in peers {
        let mut tags = peer.tags;
        let Some(names) = tags.as_array_mut() else {
            continue;
        };
        if !names
            .iter()
            .any(|name| name.as_str() == Some(model.name.as_str()))
        {
            continue;
        }
        if let Some(replacement) = replacement {
            for name in names {
                if name.as_str() == Some(model.name.as_str()) {
                    *name = serde_json::Value::String(replacement.to_string());
                }
            }
        } else {
            names.retain(|name| name.as_str() != Some(model.name.as_str()));
        }
        address_book::ActiveModel {
            row_id: Set(peer.row_id),
            tags: Set(tags),
            updated_at: Set(now()),
            ..Default::default()
        }
        .update(transaction)
        .await?;
    }
    Ok(())
}

fn rustdesk_named_tag_color(name: &str) -> Option<i64> {
    match name.to_lowercase().as_str() {
        "red" => Some(0xfff44336),
        "green" => Some(0xff4caf50),
        "blue" => Some(0xff2196f3),
        "orange" => Some(0xffff9800),
        "purple" => Some(0xff9c27b0),
        "grey" => Some(0xff9e9e9e),
        "cyan" => Some(0xff00bcd4),
        "lime" => Some(0xffcddc39),
        "teal" => Some(0xff009688),
        "pink" => Some(0xfff48fb1),
        "indigo" => Some(0xff3f51b5),
        "brown" => Some(0xff795548),
        "yellow" => Some(0xffffff00),
        _ => None,
    }
}

fn rustdesk_tag_palette() -> &'static [i64] {
    &[
        0xfff44336, 0xff4caf50, 0xff2196f3, 0xffff9800, 0xff9c27b0, 0xff9e9e9e, 0xff00bcd4,
        0xffcddc39, 0xff009688, 0xfff48fb1, 0xff3f51b5, 0xff795548,
    ]
}

pub fn default_color_for_name(name: &str, existing: &[i64]) -> i64 {
    if let Some(color) = rustdesk_named_tag_color(name) {
        return color;
    }
    let palette = rustdesk_tag_palette();
    let hash = name.chars().map(|ch| ch as usize).sum::<usize>();
    let mut color = palette[hash % palette.len()];
    if existing.contains(&color) {
        if let Some(not_used) = palette
            .iter()
            .find(|candidate| !existing.contains(candidate))
        {
            color = *not_used;
        }
    }
    color
}

pub async fn ensure_names(
    db: &DatabaseConnection,
    user_id: i32,
    collection_id: i32,
    names: &[String],
) -> Result<(), DbErr> {
    if user_id <= 0 {
        return Ok(());
    }
    if !names.iter().any(|name| !name.trim().is_empty()) {
        return Ok(());
    }
    let existing = list_by_user_and_collection(db, user_id, collection_id).await?;
    let mut existing_names: HashSet<String> = existing.iter().map(|t| t.name.clone()).collect();
    let mut existing_colors: Vec<i64> = existing.iter().map(|t| t.color).collect();
    let mut seen = HashSet::new();
    for raw in names {
        let name = raw.trim();
        if name.is_empty() || !seen.insert(name.to_string()) || existing_names.contains(name) {
            continue;
        }
        let color = default_color_for_name(name, &existing_colors);
        create(db, name, color, user_id, collection_id).await?;
        existing_names.insert(name.to_string());
        existing_colors.push(color);
    }
    Ok(())
}

/// Reconcile a user's legacy personal tags against `name -> color`.
pub async fn update_tags(
    db: &DatabaseConnection,
    user_id: i32,
    tags: std::collections::HashMap<String, i64>,
) -> Result<(), DbErr> {
    update_tags_collection(db, user_id, 0, tags).await
}

pub async fn update_tags_collection(
    db: &DatabaseConnection,
    user_id: i32,
    collection_id: i32,
    mut tags: std::collections::HashMap<String, i64>,
) -> Result<(), DbErr> {
    let existing = list_by_user_and_collection(db, user_id, collection_id).await?;
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
        create(db, &name, color, user_id, collection_id).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectionTrait, Database, DbBackend, Schema};

    async fn setup_peer_tags_db() -> DatabaseConnection {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let schema = Schema::new(DbBackend::Sqlite);
        for statement in [
            schema.create_table_from_entity(tag::Entity),
            schema.create_table_from_entity(address_book::Entity),
        ] {
            db.execute(db.get_database_backend().build(&statement))
                .await
                .unwrap();
        }
        for (user_id, collection_id) in [(1, 2), (1, 3), (2, 2)] {
            create(&db, "old", 42, user_id, collection_id)
                .await
                .unwrap();
            address_book::ActiveModel {
                user_id: Set(user_id),
                collection_id: Set(collection_id),
                tags: Set(serde_json::json!(["old", "older", "keep"])),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }
        db
    }

    #[tokio::test]
    async fn rename_updates_only_same_owner_and_book_peer_tags() {
        let db = setup_peer_tags_db().await;
        let tag = info_by_user_name_collection(&db, 1, "old", 2)
            .await
            .unwrap()
            .unwrap();
        rename_with_peers(&db, &tag, "new").await.unwrap();
        let renamed = info_by_id(&db, tag.id).await.unwrap().unwrap();
        assert_eq!(renamed.name, "new");
        assert_eq!(renamed.color, 42);
        for peer in address_book::Entity::find().all(&db).await.unwrap() {
            let expected = if peer.user_id == 1 && peer.collection_id == 2 {
                serde_json::json!(["new", "older", "keep"])
            } else {
                serde_json::json!(["old", "older", "keep"])
            };
            assert_eq!(peer.tags, expected);
        }
        assert_eq!(
            tag::Entity::find()
                .filter(tag::Column::Name.eq("old"))
                .count(&db)
                .await
                .unwrap(),
            2
        );
    }

    #[tokio::test]
    async fn delete_updates_only_same_owner_and_book_peer_tags() {
        let db = setup_peer_tags_db().await;
        let tag = info_by_user_name_collection(&db, 1, "old", 2)
            .await
            .unwrap()
            .unwrap();
        delete_with_peers(&db, &tag).await.unwrap();
        assert!(info_by_id(&db, tag.id).await.unwrap().is_none());
        for peer in address_book::Entity::find().all(&db).await.unwrap() {
            let expected = if peer.user_id == 1 && peer.collection_id == 2 {
                serde_json::json!(["older", "keep"])
            } else {
                serde_json::json!(["old", "older", "keep"])
            };
            assert_eq!(peer.tags, expected);
        }
        assert_eq!(tag::Entity::find().count(&db).await.unwrap(), 2);
    }

    #[tokio::test]
    async fn peer_update_failure_rolls_back_tag_rename_and_delete() {
        for delete_tag in [false, true] {
            let db = setup_peer_tags_db().await;
            db.execute(Statement::from_string(
                DbBackend::Sqlite,
                "CREATE TRIGGER reject_peer_tags BEFORE UPDATE OF tags ON address_books \
                 BEGIN SELECT RAISE(ABORT, 'peer update failed'); END",
            ))
            .await
            .unwrap();
            let tag = info_by_user_name_collection(&db, 1, "old", 2)
                .await
                .unwrap()
                .unwrap();
            let result = if delete_tag {
                delete_with_peers(&db, &tag).await
            } else {
                rename_with_peers(&db, &tag, "new").await
            };
            assert!(result.is_err());
            assert_eq!(info_by_id(&db, tag.id).await.unwrap(), Some(tag));
            for peer in address_book::Entity::find().all(&db).await.unwrap() {
                assert_eq!(peer.tags, serde_json::json!(["old", "older", "keep"]));
            }
        }
    }

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

    #[tokio::test]
    async fn legacy_sync_preserves_other_collection_tags() {
        let db = setup_peer_tags_db().await;
        create(&db, "old", 42, 1, 0).await.unwrap();
        update_tags(&db, 1, std::collections::HashMap::new())
            .await
            .unwrap();

        assert!(list_by_user_and_collection(&db, 1, 0)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            list_by_user_and_collection(&db, 1, 2).await.unwrap().len(),
            1
        );
        assert_eq!(
            list_by_user_and_collection(&db, 1, 3).await.unwrap().len(),
            1
        );
        assert_eq!(
            list_by_user_and_collection(&db, 2, 2).await.unwrap().len(),
            1
        );
    }

    #[tokio::test]
    async fn collection_sync_updates_and_creates_only_selected_collection_tags() {
        let db = setup_peer_tags_db().await;
        update_tags_collection(
            &db,
            1,
            2,
            std::collections::HashMap::from([("old".to_string(), 99), ("new".to_string(), 100)]),
        )
        .await
        .unwrap();

        let tags = list_by_user_and_collection(&db, 1, 2).await.unwrap();
        assert_eq!(tags.len(), 2);
        assert!(tags.iter().any(|tag| tag.name == "old" && tag.color == 99));
        assert!(tags.iter().any(|tag| tag.name == "new" && tag.color == 100));
        assert!(list_by_user_and_collection(&db, 1, 0)
            .await
            .unwrap()
            .is_empty());
        for (user_id, collection_id) in [(1, 3), (2, 2)] {
            let tags = list_by_user_and_collection(&db, user_id, collection_id)
                .await
                .unwrap();
            assert_eq!(tags.len(), 1);
            assert_eq!(tags[0].name, "old");
            assert_eq!(tags[0].color, 42);
        }
    }

    #[tokio::test]
    async fn ensure_names_creates_missing_tags_for_collection() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let schema = Schema::new(DbBackend::Sqlite);
        db.execute(
            db.get_database_backend()
                .build(&schema.create_table_from_entity(tag::Entity)),
        )
        .await
        .unwrap();

        create(&db, "windows", 0xff2196f3, 1, 2).await.unwrap();
        ensure_names(
            &db,
            1,
            2,
            &[
                "windows".to_string(),
                "android".to_string(),
                "android".to_string(),
            ],
        )
        .await
        .unwrap();

        let tags = list_by_user_and_collection(&db, 1, 2).await.unwrap();
        assert_eq!(tags.len(), 2);
        assert!(tags.iter().any(|t| t.name == "windows"));
        assert!(tags.iter().any(|t| t.name == "android"));
    }
}
