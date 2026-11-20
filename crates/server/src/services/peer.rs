//! Peer service — ports `service/peer.go`.

use sea_orm::*;

use ::entity::peer;

use crate::services::{now, paginate};

pub async fn find_by_id(db: &DatabaseConnection, id: &str) -> Result<Option<peer::Model>, DbErr> {
    peer::Entity::find()
        .filter(peer::Column::Id.eq(id))
        .one(db)
        .await
}

pub async fn find_by_uuid(
    db: &DatabaseConnection,
    uuid: &str,
) -> Result<Option<peer::Model>, DbErr> {
    peer::Entity::find()
        .filter(peer::Column::Uuid.eq(uuid))
        .one(db)
        .await
}

pub async fn info_by_row_id(
    db: &DatabaseConnection,
    row_id: i32,
) -> Result<Option<peer::Model>, DbErr> {
    peer::Entity::find_by_id(row_id).one(db).await
}

pub async fn erase_user_id(db: &DatabaseConnection, user_id: i32) -> Result<(), DbErr> {
    let peers = peer::Entity::find()
        .filter(peer::Column::UserId.eq(user_id))
        .all(db)
        .await?;
    for p in peers {
        let mut am: peer::ActiveModel = p.into();
        am.user_id = Set(0);
        am.update(db).await?;
    }
    Ok(())
}

pub struct PeerListResult {
    pub list: Vec<peer::Model>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

pub async fn list_by_user_ids(
    db: &DatabaseConnection,
    user_ids: &[i32],
    page: u64,
    page_size: u64,
) -> Result<PeerListResult, DbErr> {
    let (page, page_size) = paginate(page, page_size);
    let q = peer::Entity::find().filter(peer::Column::UserId.is_in(user_ids.to_vec()));
    let total = q.clone().count(db).await? as i64;
    let list = q
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;
    Ok(PeerListResult {
        list,
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

pub async fn list(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
    id_like: Option<String>,
) -> Result<PeerListResult, DbErr> {
    let (page, page_size) = paginate(page, page_size);
    let mut q = peer::Entity::find();
    if let Some(id) = id_like.filter(|s| !s.is_empty()) {
        q = q.filter(peer::Column::Id.contains(&id));
    }
    let total = q.clone().count(db).await? as i64;
    let list = q
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;
    Ok(PeerListResult {
        list,
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

fn existing_groups(ids: &[i32]) -> sea_orm::sea_query::SelectStatement {
    use ::entity::device_group;
    sea_orm::sea_query::Query::select()
        .column(device_group::Column::Id)
        .from(device_group::Entity)
        .and_where(device_group::Column::Id.is_in(ids.iter().copied()))
        .to_owned()
}

pub(crate) async fn detail_in_groups(
    db: &DatabaseConnection,
    row_id: i32,
    ids: &[i32],
) -> Result<Option<peer::Model>, DbErr> {
    peer::Entity::find_by_id(row_id)
        .filter(peer::Column::GroupId.in_subquery(existing_groups(ids)))
        .one(db)
        .await
}

/// Apply an operator's edit with the membership predicate on the database write.
pub(crate) async fn update_in_groups(
    db: &DatabaseConnection,
    actor_id: i32,
    row_id: i32,
    alias: Option<String>,
    destination: Option<i32>,
) -> Result<bool, DbErr> {
    use super::console_roles::{self, ConsoleRole};
    use sea_orm::sea_query::Expr;

    let transaction = console_roles::authority_transaction(db).await?;
    let actor = ::entity::user::Entity::find_by_id(actor_id)
        .one(&transaction)
        .await?;
    if !actor.is_some_and(|actor| actor.is_enabled() && !actor.is_admin()) {
        return Ok(false);
    }
    let ConsoleRole::DeviceGroupOperator { group_ids } =
        console_roles::for_user(&transaction, actor_id).await?
    else {
        return Ok(false);
    };
    if let Some(id) = destination {
        if !group_ids.contains(&id)
            || ::entity::device_group::Entity::find_by_id(id)
                .one(&transaction)
                .await?
                .is_none()
        {
            return Ok(false);
        }
    }
    let mut update = peer::Entity::update_many()
        .col_expr(peer::Column::UpdatedAt, Expr::value(now()))
        .filter(peer::Column::RowId.eq(row_id))
        .filter(peer::Column::GroupId.in_subquery(existing_groups(&group_ids)));
    if let Some(alias) = alias {
        update = update.col_expr(peer::Column::Alias, Expr::value(alias));
    }
    if let Some(group_id) = destination {
        update = update.col_expr(peer::Column::GroupId, Expr::value(group_id));
    }
    let changed = update.exec(&transaction).await?.rows_affected > 0;
    transaction.commit().await?;
    Ok(changed)
}

pub async fn create(db: &DatabaseConnection, am: peer::ActiveModel) -> Result<peer::Model, DbErr> {
    let mut am = am;
    am.created_at = Set(now());
    am.updated_at = Set(now());
    am.insert(db).await
}

pub async fn update(db: &DatabaseConnection, am: peer::ActiveModel) -> Result<(), DbErr> {
    let mut am = am;
    am.updated_at = Set(now());
    am.update(db).await?;
    Ok(())
}

pub async fn delete(db: &DatabaseConnection, p: &peer::Model) -> Result<(), DbErr> {
    let uuid = p.uuid.clone();
    peer::Entity::delete_by_id(p.row_id).exec(db).await?;
    if !uuid.is_empty() {
        crate::services::user::flush_token_by_uuid(db, &uuid).await?;
    }
    Ok(())
}

pub async fn batch_delete(db: &DatabaseConnection, ids: &[i32]) -> Result<(), DbErr> {
    let uuids: Vec<String> = peer::Entity::find()
        .filter(peer::Column::RowId.is_in(ids.to_vec()))
        .all(db)
        .await?
        .into_iter()
        .map(|p| p.uuid)
        .filter(|u| !u.is_empty())
        .collect();
    peer::Entity::delete_many()
        .filter(peer::Column::RowId.is_in(ids.to_vec()))
        .exec(db)
        .await?;
    for uuid in uuids {
        crate::services::user::flush_token_by_uuid(db, &uuid).await?;
    }
    Ok(())
}

/// Filters for the admin/my peer list (≈ `admin.PeerQuery`).
#[derive(Default)]
pub struct PeerFilters {
    pub user_id: Option<i32>,
    pub time_ago: i64,
    pub id: Option<String>,
    pub exact_id: bool,
    pub hostname: Option<String>,
    pub username: Option<String>,
    pub ip: Option<String>,
    pub alias: Option<String>,
}

pub async fn list_filtered(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
    f: PeerFilters,
) -> Result<PeerListResult, DbErr> {
    list_filtered_in_groups(db, page, page_size, f, None).await
}

pub(crate) async fn list_filtered_in_groups(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
    f: PeerFilters,
    group_ids: Option<&[i32]>,
) -> Result<PeerListResult, DbErr> {
    let (page, page_size) = paginate(page, page_size);
    let mut q = peer::Entity::find();
    if let Some(ids) = group_ids {
        q = q.filter(peer::Column::GroupId.in_subquery(existing_groups(ids)));
    }
    if let Some(uid) = f.user_id {
        q = q.filter(peer::Column::UserId.eq(uid));
    }
    if f.time_ago > 0 {
        let lt = chrono::Utc::now().timestamp() - f.time_ago;
        q = q.filter(peer::Column::LastOnlineTime.lt(lt));
    } else if f.time_ago < 0 {
        let gt = chrono::Utc::now().timestamp() + f.time_ago;
        q = q.filter(peer::Column::LastOnlineTime.gt(gt));
    }
    if let Some(v) = f.id.filter(|s| !s.is_empty()) {
        q = q.filter(if f.exact_id {
            peer::Column::Id.eq(v)
        } else {
            peer::Column::Id.contains(&v)
        });
    }
    if let Some(v) = f.hostname.filter(|s| !s.is_empty()) {
        q = q.filter(peer::Column::Hostname.contains(&v));
    }
    if let Some(v) = f.username.filter(|s| !s.is_empty()) {
        q = q.filter(peer::Column::Username.contains(&v));
    }
    if let Some(v) = f.ip.filter(|s| !s.is_empty()) {
        q = q.filter(peer::Column::LastOnlineIp.contains(&v));
    }
    if let Some(v) = f.alias.filter(|s| !s.is_empty()) {
        q = q.filter(peer::Column::Alias.contains(&v));
    }
    let total = q.clone().count(db).await? as i64;
    let list = q
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;
    Ok(PeerListResult {
        list,
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

/// Fetch peers by row ids (used by batch-create-from-peers).
pub async fn list_by_row_ids(
    db: &DatabaseConnection,
    row_ids: &[i32],
) -> Result<Vec<peer::Model>, DbErr> {
    if row_ids.is_empty() {
        return Ok(vec![]);
    }
    peer::Entity::find()
        .filter(peer::Column::RowId.is_in(row_ids.to_vec()))
        .all(db)
        .await
}

/// Public "simple data" for given ids — only id + version (≈ `SimpleData`).
pub async fn simple_data(
    db: &DatabaseConnection,
    ids: &[String],
) -> Result<Vec<serde_json::Value>, DbErr> {
    let peers = peer::Entity::find()
        .filter(peer::Column::Id.is_in(ids.to_vec()))
        .all(db)
        .await?;
    Ok(peers
        .into_iter()
        .map(|p| serde_json::json!({ "id": p.id, "version": p.version }))
        .collect())
}
