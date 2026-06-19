//! Login log service — ports the read side of `service/loginLog.go`.

use sea_orm::*;

use ::entity::login_log;

use crate::services::paginate;

pub struct LoginLogListResult {
    pub list: Vec<login_log::Model>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

#[derive(Default)]
pub struct LoginLogFilters {
    pub user_id: Option<i32>,
    pub client: Option<String>,
    pub login_type: Option<String>,
}

pub async fn list(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
    filters: LoginLogFilters,
) -> Result<LoginLogListResult, DbErr> {
    let (page, page_size) = paginate(page, page_size);
    let mut q = login_log::Entity::find().filter(login_log::Column::IsDeleted.eq(0));
    if let Some(uid) = filters.user_id {
        q = q.filter(login_log::Column::UserId.eq(uid));
    }
    if let Some(client) = filters.client.and_then(non_empty) {
        q = q.filter(login_log::Column::Client.eq(client));
    }
    if let Some(login_type) = filters.login_type.and_then(non_empty) {
        q = q.filter(login_log::Column::Type.eq(login_type));
    }
    let total = q.clone().count(db).await? as i64;
    let list = q
        .order_by_desc(login_log::Column::Id)
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;
    Ok(LoginLogListResult {
        list,
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

fn non_empty(value: String) -> Option<String> {
    let value = value.trim().to_string();
    (!value.is_empty()).then_some(value)
}

pub async fn info_by_id(
    db: &DatabaseConnection,
    id: i32,
) -> Result<Option<login_log::Model>, DbErr> {
    login_log::Entity::find_by_id(id).one(db).await
}

/// Hard delete (admin).
pub async fn delete(db: &DatabaseConnection, id: i32) -> Result<(), DbErr> {
    login_log::Entity::delete_by_id(id).exec(db).await?;
    Ok(())
}

pub async fn batch_delete(db: &DatabaseConnection, ids: &[i32]) -> Result<(), DbErr> {
    login_log::Entity::delete_many()
        .filter(login_log::Column::Id.is_in(ids.to_vec()))
        .exec(db)
        .await?;
    Ok(())
}

/// Soft delete (set is_deleted) — used by the `my/*` endpoints.
pub async fn soft_delete(db: &DatabaseConnection, id: i32) -> Result<(), DbErr> {
    let am = login_log::ActiveModel {
        id: Set(id),
        is_deleted: Set(1),
        ..Default::default()
    };
    am.update(db).await?;
    Ok(())
}

pub async fn batch_soft_delete(
    db: &DatabaseConnection,
    user_id: i32,
    ids: &[i32],
) -> Result<(), DbErr> {
    login_log::Entity::update_many()
        .col_expr(
            login_log::Column::IsDeleted,
            sea_orm::sea_query::Expr::value(1),
        )
        .filter(login_log::Column::UserId.eq(user_id))
        .filter(login_log::Column::Id.is_in(ids.to_vec()))
        .exec(db)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::now;
    use sea_orm::{ConnectionTrait, Database, DbBackend, Schema, Set};

    async fn setup_db() -> DatabaseConnection {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let schema = Schema::new(DbBackend::Sqlite);
        db.execute(
            db.get_database_backend()
                .build(&schema.create_table_from_entity(login_log::Entity)),
        )
        .await
        .unwrap();
        db
    }

    async fn insert_login_log(
        db: &DatabaseConnection,
        id: i32,
        user_id: i32,
        client: &str,
        login_type: &str,
    ) {
        login_log::ActiveModel {
            id: Set(id),
            user_id: Set(user_id),
            client: Set(client.to_string()),
            r#type: Set(login_type.to_string()),
            device_id: Set(format!("device-{id}")),
            created_at: Set(now()),
            updated_at: Set(now()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn list_filters_login_logs_by_user_client_and_type() {
        let db = setup_db().await;
        insert_login_log(
            &db,
            1,
            1,
            login_log::CLIENT_WEB_ADMIN,
            login_log::TYPE_ACCOUNT,
        )
        .await;
        insert_login_log(&db, 2, 1, login_log::CLIENT_WEB, login_log::TYPE_ACCOUNT).await;
        insert_login_log(&db, 3, 2, login_log::CLIENT_APP, login_log::TYPE_OAUTH).await;

        let user_rows = list(
            &db,
            1,
            10,
            LoginLogFilters {
                user_id: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(user_rows.total, 2);

        let webclient_rows = list(
            &db,
            1,
            10,
            LoginLogFilters {
                client: Some(login_log::CLIENT_WEB.to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(webclient_rows.total, 1);
        assert_eq!(webclient_rows.list[0].id, 2);

        let oauth_rows = list(
            &db,
            1,
            10,
            LoginLogFilters {
                login_type: Some(login_log::TYPE_OAUTH.to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(oauth_rows.total, 1);
        assert_eq!(oauth_rows.list[0].id, 3);

        let no_rows = list(
            &db,
            1,
            10,
            LoginLogFilters {
                user_id: Some(1),
                login_type: Some(login_log::TYPE_OAUTH.to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(no_rows.total, 0);
    }

    #[tokio::test]
    async fn list_ignores_blank_filter_values() {
        let db = setup_db().await;
        insert_login_log(
            &db,
            1,
            1,
            login_log::CLIENT_WEB_ADMIN,
            login_log::TYPE_ACCOUNT,
        )
        .await;
        insert_login_log(&db, 2, 2, login_log::CLIENT_APP, login_log::TYPE_OAUTH).await;

        let rows = list(
            &db,
            1,
            10,
            LoginLogFilters {
                client: Some("  ".to_string()),
                login_type: Some("".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(rows.total, 2);
    }
}
