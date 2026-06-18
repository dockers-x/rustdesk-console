//! Address book service — ports `service/addressBook.go` (Phase 1 subset:
//! personal address books, tags, collections, share rules, sharing).

use std::collections::HashMap;

use sea_orm::sea_query::Expr;
use sea_orm::*;
use serde_json::Value;

use ::entity::{
    address_book, address_book_collection, address_book_collection_rule, share_record, tag,
};

use crate::services::now;

pub fn platform_from_os(os: &str) -> String {
    let l = os.to_lowercase();
    if l.contains("android") {
        "Android".into()
    } else if l.contains("windows") {
        "Windows".into()
    } else if l.contains("linux") {
        "Linux".into()
    } else if l.contains("mac") {
        "Mac OS".into()
    } else {
        String::new()
    }
}

pub async fn list_by_user_and_collection(
    db: &DatabaseConnection,
    user_id: i32,
    collection_id: i32,
) -> Result<Vec<address_book::Model>, DbErr> {
    address_book::Entity::find()
        .filter(address_book::Column::UserId.eq(user_id))
        .filter(address_book::Column::CollectionId.eq(collection_id))
        .all(db)
        .await
}

pub async fn info_by_user_id_and_id(
    db: &DatabaseConnection,
    user_id: i32,
    id: &str,
) -> Result<Option<address_book::Model>, DbErr> {
    address_book::Entity::find()
        .filter(address_book::Column::UserId.eq(user_id))
        .filter(address_book::Column::Id.eq(id))
        .one(db)
        .await
}

pub async fn info_by_user_id_and_id_and_cid(
    db: &DatabaseConnection,
    user_id: i32,
    id: &str,
    collection_id: i32,
) -> Result<Option<address_book::Model>, DbErr> {
    address_book::Entity::find()
        .filter(address_book::Column::UserId.eq(user_id))
        .filter(address_book::Column::Id.eq(id))
        .filter(address_book::Column::CollectionId.eq(collection_id))
        .one(db)
        .await
}

pub async fn aliases_by_user_and_ids(
    db: &DatabaseConnection,
    user_id: i32,
    ids: &[String],
) -> Result<HashMap<String, String>, DbErr> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    let rows = address_book::Entity::find()
        .filter(address_book::Column::UserId.eq(user_id))
        .filter(address_book::Column::Id.is_in(ids.to_vec()))
        .filter(address_book::Column::Alias.ne(""))
        .order_by_desc(address_book::Column::UpdatedAt)
        .order_by_desc(address_book::Column::RowId)
        .all(db)
        .await?;

    let mut aliases = HashMap::new();
    for row in rows {
        let alias = row.alias.trim().to_string();
        if !alias.is_empty() {
            aliases.entry(row.id).or_insert(alias);
        }
    }
    Ok(aliases)
}

pub async fn add(
    db: &DatabaseConnection,
    am: address_book::ActiveModel,
) -> Result<address_book::Model, DbErr> {
    let mut am = am;
    am.created_at = Set(now());
    am.updated_at = Set(now());
    am.insert(db).await
}

pub async fn delete(db: &DatabaseConnection, row_id: i32) -> Result<(), DbErr> {
    address_book::Entity::delete_by_id(row_id).exec(db).await?;
    Ok(())
}

/// Apply allowed-field updates to an address book peer (≈ `UpdateByMap`).
pub async fn update_fields(
    db: &DatabaseConnection,
    model: &address_book::Model,
    fields: &Value,
) -> Result<(), DbErr> {
    let mut am: address_book::ActiveModel = model.clone().into();
    if let Some(obj) = fields.as_object() {
        if let Some(v) = obj.get("password").and_then(|v| v.as_str()) {
            am.password = Set(v.to_string());
        }
        if let Some(v) = obj.get("hash").and_then(|v| v.as_str()) {
            am.hash = Set(v.to_string());
        }
        if let Some(v) = obj.get("alias").and_then(|v| v.as_str()) {
            am.alias = Set(v.to_string());
        }
        if let Some(v) = obj.get("tags") {
            am.tags = Set(v.clone());
        }
    }
    am.updated_at = Set(now());
    am.update(db).await?;
    Ok(())
}

/// Reconcile a user's whole personal address book against `peers` (≈ `UpdateAddressBook`).
pub async fn update_address_book(
    db: &DatabaseConnection,
    peers: Vec<address_book::Model>,
    user_id: i32,
) -> Result<(), DbErr> {
    let existing = address_book::Entity::find()
        .filter(address_book::Column::UserId.eq(user_id))
        .all(db)
        .await?;
    let existing_by_id: HashMap<String, address_book::Model> =
        existing.into_iter().map(|m| (m.id.clone(), m)).collect();
    let incoming_ids: std::collections::HashSet<String> =
        peers.iter().map(|p| p.id.clone()).collect();

    for mut p in peers {
        p.user_id = user_id;
        match existing_by_id.get(&p.id) {
            None => {
                if p.platform.is_empty() || p.username.is_empty() || p.hostname.is_empty() {
                    if let Some(peer) = crate::services::peer::find_by_id(db, &p.id).await? {
                        p.platform = platform_from_os(&peer.os);
                        p.username = peer.username;
                        p.hostname = peer.hostname;
                    }
                }
                let am = address_book::ActiveModel {
                    id: Set(p.id),
                    username: Set(p.username),
                    password: Set(p.password),
                    hostname: Set(p.hostname),
                    alias: Set(p.alias),
                    platform: Set(p.platform),
                    tags: Set(p.tags),
                    hash: Set(p.hash),
                    user_id: Set(user_id),
                    force_always_relay: Set(p.force_always_relay),
                    rdp_port: Set(p.rdp_port),
                    rdp_username: Set(p.rdp_username),
                    online: Set(p.online),
                    login_name: Set(p.login_name),
                    same_server: Set(p.same_server),
                    collection_id: Set(p.collection_id),
                    created_at: Set(now()),
                    updated_at: Set(now()),
                    ..Default::default()
                };
                am.insert(db).await?;
            }
            Some(existing) => {
                let am = address_book::ActiveModel {
                    row_id: Set(existing.row_id),
                    username: Set(p.username),
                    password: Set(p.password),
                    hostname: Set(p.hostname),
                    alias: Set(p.alias),
                    platform: Set(p.platform),
                    tags: Set(p.tags),
                    hash: Set(p.hash),
                    force_always_relay: Set(p.force_always_relay),
                    rdp_port: Set(p.rdp_port),
                    rdp_username: Set(p.rdp_username),
                    online: Set(p.online),
                    login_name: Set(p.login_name),
                    same_server: Set(p.same_server),
                    updated_at: Set(now()),
                    ..Default::default()
                };
                am.update(db).await?;
            }
        }
    }
    // delete those not present anymore
    for (id, model) in existing_by_id {
        if !incoming_ids.contains(&id) {
            address_book::Entity::delete_by_id(model.row_id)
                .exec(db)
                .await?;
        }
    }
    Ok(())
}

// --- collections ---

pub async fn collection_info_by_id(
    db: &DatabaseConnection,
    id: i32,
) -> Result<Option<address_book_collection::Model>, DbErr> {
    address_book_collection::Entity::find_by_id(id)
        .one(db)
        .await
}

pub async fn collection_info_by_user_and_name(
    db: &DatabaseConnection,
    user_id: i32,
    name: &str,
) -> Result<Option<address_book_collection::Model>, DbErr> {
    address_book_collection::Entity::find()
        .filter(address_book_collection::Column::UserId.eq(user_id))
        .filter(address_book_collection::Column::Name.eq(name))
        .one(db)
        .await
}

pub async fn collection_list_by_user_id(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<Vec<address_book_collection::Model>, DbErr> {
    address_book_collection::Entity::find()
        .filter(address_book_collection::Column::UserId.eq(user_id))
        .all(db)
        .await
}

pub async fn collection_list_by_ids(
    db: &DatabaseConnection,
    ids: &[i32],
) -> Result<Vec<address_book_collection::Model>, DbErr> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    address_book_collection::Entity::find()
        .filter(address_book_collection::Column::Id.is_in(ids.to_vec()))
        .all(db)
        .await
}

pub async fn transfer_collection_owner(
    db: &DatabaseConnection,
    collection_id: i32,
    old_user_id: i32,
    new_user_id: i32,
) -> Result<(), DbErr> {
    if old_user_id == new_user_id {
        return Ok(());
    }
    address_book::Entity::update_many()
        .col_expr(address_book::Column::UserId, Expr::value(new_user_id))
        .filter(address_book::Column::CollectionId.eq(collection_id))
        .filter(address_book::Column::UserId.eq(old_user_id))
        .exec(db)
        .await?;
    tag::Entity::update_many()
        .col_expr(tag::Column::UserId, Expr::value(new_user_id))
        .filter(tag::Column::CollectionId.eq(collection_id))
        .filter(tag::Column::UserId.eq(old_user_id))
        .exec(db)
        .await?;
    address_book_collection_rule::Entity::update_many()
        .col_expr(
            address_book_collection_rule::Column::UserId,
            Expr::value(new_user_id),
        )
        .filter(address_book_collection_rule::Column::CollectionId.eq(collection_id))
        .filter(address_book_collection_rule::Column::UserId.eq(old_user_id))
        .exec(db)
        .await?;
    Ok(())
}

// --- share rules / privileges ---

pub async fn collection_read_rules(
    db: &DatabaseConnection,
    user_id: i32,
    group_id: i32,
) -> Result<Vec<address_book_collection_rule::Model>, DbErr> {
    use address_book_collection_rule as r;
    let mut res = r::Entity::find()
        .filter(r::Column::Type.eq(r::RULE_TYPE_PERSONAL))
        .filter(r::Column::ToId.eq(user_id))
        .filter(r::Column::Rule.gt(0))
        .all(db)
        .await?;
    let group = r::Entity::find()
        .filter(r::Column::Type.eq(r::RULE_TYPE_GROUP))
        .filter(r::Column::ToId.is_in(vec![group_id, 0]))
        .filter(r::Column::Rule.gt(0))
        .all(db)
        .await?;
    res.extend(group);
    Ok(res)
}

/// Maximum rule level a user has on (owner uid, collection cid).
pub async fn user_max_rule(
    db: &DatabaseConnection,
    cur_user_id: i32,
    cur_group_id: i32,
    uid: i32,
    cid: i32,
) -> Result<i32, DbErr> {
    use address_book_collection_rule as r;
    if cur_user_id == uid {
        return Ok(r::RULE_FULL_CONTROL);
    }
    let mut max = 0;
    if let Some(p) = r::Entity::find()
        .filter(r::Column::Type.eq(r::RULE_TYPE_PERSONAL))
        .filter(r::Column::CollectionId.eq(cid))
        .filter(r::Column::ToId.eq(cur_user_id))
        .one(db)
        .await?
    {
        max = p.rule;
        if max == r::RULE_FULL_CONTROL {
            return Ok(max);
        }
    }
    if let Some(g) = r::Entity::find()
        .filter(r::Column::Type.eq(r::RULE_TYPE_GROUP))
        .filter(r::Column::CollectionId.eq(cid))
        .filter(r::Column::ToId.is_in(vec![cur_group_id, 0]))
        .order_by_desc(r::Column::Rule)
        .one(db)
        .await?
    {
        if g.rule > max {
            max = g.rule;
        }
    }
    Ok(max)
}

pub async fn can_read(
    db: &DatabaseConnection,
    cur_user_id: i32,
    cur_group_id: i32,
    uid: i32,
    cid: i32,
) -> Result<bool, DbErr> {
    Ok(
        user_max_rule(db, cur_user_id, cur_group_id, uid, cid).await?
            >= address_book_collection_rule::RULE_READ,
    )
}

pub async fn can_write(
    db: &DatabaseConnection,
    cur_user_id: i32,
    cur_group_id: i32,
    uid: i32,
    cid: i32,
) -> Result<bool, DbErr> {
    Ok(
        user_max_rule(db, cur_user_id, cur_group_id, uid, cid).await?
            >= address_book_collection_rule::RULE_READ_WRITE,
    )
}

pub async fn can_full_control(
    db: &DatabaseConnection,
    cur_user_id: i32,
    cur_group_id: i32,
    uid: i32,
    cid: i32,
) -> Result<bool, DbErr> {
    Ok(
        user_max_rule(db, cur_user_id, cur_group_id, uid, cid).await?
            >= address_book_collection_rule::RULE_FULL_CONTROL,
    )
}

// --- sharing ---

pub async fn shared_peer(
    db: &DatabaseConnection,
    share_token: &str,
) -> Result<Option<share_record::Model>, DbErr> {
    share_record::Entity::find()
        .filter(share_record::Column::ShareToken.eq(share_token))
        .one(db)
        .await
}

// ---- admin address book CRUD ----

pub struct AddressBookListResult {
    pub list: Vec<address_book::Model>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

#[derive(Default)]
pub struct AbFilters {
    pub id: Option<String>,
    pub user_id: Option<i32>,
    pub username: Option<String>,
    pub hostname: Option<String>,
    pub collection_id: Option<i32>,
}

pub async fn admin_list(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
    f: AbFilters,
) -> Result<AddressBookListResult, DbErr> {
    let (page, page_size) = crate::services::paginate(page, page_size);
    let mut q = address_book::Entity::find();
    if let Some(v) = f.id.filter(|s| !s.is_empty()) {
        q = q.filter(address_book::Column::Id.contains(&v));
    }
    if let Some(v) = f.user_id.filter(|v| *v > 0) {
        q = q.filter(address_book::Column::UserId.eq(v));
    }
    if let Some(v) = f.username.filter(|s| !s.is_empty()) {
        q = q.filter(address_book::Column::Username.contains(&v));
    }
    if let Some(v) = f.hostname.filter(|s| !s.is_empty()) {
        q = q.filter(address_book::Column::Hostname.contains(&v));
    }
    if let Some(v) = f.collection_id.filter(|v| *v >= 0) {
        q = q.filter(address_book::Column::CollectionId.eq(v));
    }
    let total = q.clone().count(db).await? as i64;
    let list = q
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;
    Ok(AddressBookListResult {
        list,
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

pub async fn info_by_row_id(
    db: &DatabaseConnection,
    row_id: i32,
) -> Result<Option<address_book::Model>, DbErr> {
    address_book::Entity::find_by_id(row_id).one(db).await
}

fn to_active(m: address_book::Model) -> address_book::ActiveModel {
    address_book::ActiveModel {
        id: Set(m.id),
        username: Set(m.username),
        password: Set(m.password),
        hostname: Set(m.hostname),
        alias: Set(m.alias),
        platform: Set(m.platform),
        tags: Set(m.tags),
        hash: Set(m.hash),
        user_id: Set(m.user_id),
        force_always_relay: Set(m.force_always_relay),
        rdp_port: Set(m.rdp_port),
        rdp_username: Set(m.rdp_username),
        online: Set(m.online),
        login_name: Set(m.login_name),
        same_server: Set(m.same_server),
        collection_id: Set(m.collection_id),
        ..Default::default()
    }
}

pub async fn create(
    db: &DatabaseConnection,
    m: address_book::Model,
) -> Result<address_book::Model, DbErr> {
    let mut am = to_active(m);
    am.created_at = Set(now());
    am.updated_at = Set(now());
    am.insert(db).await
}

/// Update all editable columns by row_id (≈ `UpdateAll`, omits created_at).
pub async fn update_all(db: &DatabaseConnection, m: address_book::Model) -> Result<(), DbErr> {
    let row_id = m.row_id;
    let mut am = to_active(m);
    am.row_id = Set(row_id);
    am.updated_at = Set(now());
    am.update(db).await?;
    Ok(())
}

pub async fn check_collection_owner(
    db: &DatabaseConnection,
    user_id: i32,
    collection_id: i32,
) -> Result<bool, DbErr> {
    Ok(collection_info_by_id(db, collection_id)
        .await?
        .map(|c| c.user_id == user_id)
        .unwrap_or(false))
}

/// Build an address book entry from a peer (≈ `FromPeer`).
pub fn from_peer(p: &::entity::peer::Model) -> address_book::Model {
    address_book::Model {
        row_id: 0,
        id: p.id.clone(),
        username: p.username.clone(),
        password: String::new(),
        hostname: p.hostname.clone(),
        alias: String::new(),
        platform: platform_from_os(&p.os),
        tags: serde_json::Value::Array(vec![]),
        hash: String::new(),
        user_id: p.user_id,
        force_always_relay: false,
        rdp_port: String::new(),
        rdp_username: String::new(),
        online: false,
        login_name: String::new(),
        same_server: false,
        collection_id: 0,
        created_at: None,
        updated_at: None,
    }
}

/// Set `tags` for many address books owned by the given rows (≈ `BatchUpdateTags`).
pub async fn batch_update_tags(
    db: &DatabaseConnection,
    user_id: i32,
    row_ids: &[i32],
    tags: &serde_json::Value,
) -> Result<i64, DbErr> {
    let abs = address_book::Entity::find()
        .filter(address_book::Column::RowId.is_in(row_ids.to_vec()))
        .filter(address_book::Column::UserId.eq(user_id))
        .all(db)
        .await?;
    let n = abs.len() as i64;
    for ab in abs {
        let mut am: address_book::ActiveModel = ab.into();
        am.tags = Set(tags.clone());
        am.updated_at = Set(now());
        am.update(db).await?;
    }
    Ok(n)
}

// ---- collections (admin) ----

pub struct CollectionListResult {
    pub list: Vec<address_book_collection::Model>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

pub async fn collection_list(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
    user_id: Option<i32>,
) -> Result<CollectionListResult, DbErr> {
    let (page, page_size) = crate::services::paginate(page, page_size);
    let mut q = address_book_collection::Entity::find();
    if let Some(uid) = user_id.filter(|v| *v > 0) {
        q = q.filter(address_book_collection::Column::UserId.eq(uid));
    }
    let total = q.clone().count(db).await? as i64;
    let list = q
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;
    Ok(CollectionListResult {
        list,
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

pub async fn create_collection(
    db: &DatabaseConnection,
    user_id: i32,
    name: &str,
) -> Result<address_book_collection::Model, DbErr> {
    let am = address_book_collection::ActiveModel {
        user_id: Set(user_id),
        name: Set(name.to_string()),
        created_at: Set(now()),
        updated_at: Set(now()),
        ..Default::default()
    };
    am.insert(db).await
}

pub async fn update_collection(
    db: &DatabaseConnection,
    id: i32,
    user_id: i32,
    name: &str,
) -> Result<(), DbErr> {
    let am = address_book_collection::ActiveModel {
        id: Set(id),
        user_id: Set(user_id),
        name: Set(name.to_string()),
        updated_at: Set(now()),
        ..Default::default()
    };
    am.update(db).await?;
    Ok(())
}

/// Delete a collection and its rules + address books (≈ `DeleteCollection`).
pub async fn delete_collection(db: &DatabaseConnection, id: i32) -> Result<(), DbErr> {
    address_book_collection_rule::Entity::delete_many()
        .filter(address_book_collection_rule::Column::CollectionId.eq(id))
        .exec(db)
        .await?;
    address_book::Entity::delete_many()
        .filter(address_book::Column::CollectionId.eq(id))
        .exec(db)
        .await?;
    address_book_collection::Entity::delete_by_id(id)
        .exec(db)
        .await?;
    Ok(())
}

// ---- collection rules (admin) ----

pub struct RuleListResult {
    pub list: Vec<address_book_collection_rule::Model>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

pub async fn rule_list(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
    user_id: Option<i32>,
    collection_id: Option<i32>,
) -> Result<RuleListResult, DbErr> {
    let (page, page_size) = crate::services::paginate(page, page_size);
    let mut q = address_book_collection_rule::Entity::find();
    if let Some(uid) = user_id.filter(|v| *v > 0) {
        q = q.filter(address_book_collection_rule::Column::UserId.eq(uid));
    }
    if let Some(cid) = collection_id.filter(|v| *v > 0) {
        q = q.filter(address_book_collection_rule::Column::CollectionId.eq(cid));
    }
    let total = q.clone().count(db).await? as i64;
    let list = q
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;
    Ok(RuleListResult {
        list,
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

pub async fn rule_info_by_id(
    db: &DatabaseConnection,
    id: i32,
) -> Result<Option<address_book_collection_rule::Model>, DbErr> {
    address_book_collection_rule::Entity::find_by_id(id)
        .one(db)
        .await
}

pub async fn rule_info_by_type_to_cid(
    db: &DatabaseConnection,
    rule_type: i32,
    to_id: i32,
    collection_id: i32,
) -> Result<Option<address_book_collection_rule::Model>, DbErr> {
    address_book_collection_rule::Entity::find()
        .filter(address_book_collection_rule::Column::Type.eq(rule_type))
        .filter(address_book_collection_rule::Column::ToId.eq(to_id))
        .filter(address_book_collection_rule::Column::CollectionId.eq(collection_id))
        .one(db)
        .await
}

pub async fn rules_by_collection(
    db: &DatabaseConnection,
    collection_id: i32,
    page: u64,
    page_size: u64,
) -> Result<RuleListResult, DbErr> {
    let (page, page_size) = crate::services::paginate(page, page_size);
    let q = address_book_collection_rule::Entity::find()
        .filter(address_book_collection_rule::Column::CollectionId.eq(collection_id));
    let total = q.clone().count(db).await? as i64;
    let list = q
        .order_by_desc(address_book_collection_rule::Column::Id)
        .offset((page - 1) * page_size)
        .limit(page_size)
        .all(db)
        .await?;
    Ok(RuleListResult {
        list,
        page: page as i64,
        page_size: page_size as i64,
        total,
    })
}

pub async fn create_rule(
    db: &DatabaseConnection,
    m: &address_book_collection_rule::Model,
) -> Result<address_book_collection_rule::Model, DbErr> {
    let am = address_book_collection_rule::ActiveModel {
        user_id: Set(m.user_id),
        collection_id: Set(m.collection_id),
        rule: Set(m.rule),
        r#type: Set(m.r#type),
        to_id: Set(m.to_id),
        created_at: Set(now()),
        updated_at: Set(now()),
        ..Default::default()
    };
    am.insert(db).await
}

pub async fn update_rule(
    db: &DatabaseConnection,
    m: &address_book_collection_rule::Model,
) -> Result<(), DbErr> {
    let am = address_book_collection_rule::ActiveModel {
        id: Set(m.id),
        user_id: Set(m.user_id),
        collection_id: Set(m.collection_id),
        rule: Set(m.rule),
        r#type: Set(m.r#type),
        to_id: Set(m.to_id),
        updated_at: Set(now()),
        ..Default::default()
    };
    am.update(db).await?;
    Ok(())
}

pub async fn delete_rule(db: &DatabaseConnection, id: i32) -> Result<(), DbErr> {
    address_book_collection_rule::Entity::delete_by_id(id)
        .exec(db)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectionTrait, Database, DbBackend, Schema, Set};

    async fn setup_address_book_db() -> DatabaseConnection {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let schema = Schema::new(DbBackend::Sqlite);
        db.execute(
            db.get_database_backend()
                .build(&schema.create_table_from_entity(address_book::Entity)),
        )
        .await
        .unwrap();
        db
    }

    #[tokio::test]
    async fn aliases_by_user_and_ids_prefers_latest_non_empty_alias() {
        let db = setup_address_book_db().await;

        address_book::ActiveModel {
            id: Set("100".to_string()),
            user_id: Set(1),
            alias: Set("old name".to_string()),
            tags: Set(serde_json::json!([])),
            updated_at: Set(Some(
                chrono::NaiveDate::from_ymd_opt(2026, 1, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap(),
            )),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        address_book::ActiveModel {
            id: Set("100".to_string()),
            user_id: Set(1),
            alias: Set("new name".to_string()),
            tags: Set(serde_json::json!([])),
            updated_at: Set(Some(
                chrono::NaiveDate::from_ymd_opt(2026, 1, 2)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap(),
            )),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        address_book::ActiveModel {
            id: Set("100".to_string()),
            user_id: Set(2),
            alias: Set("other user".to_string()),
            tags: Set(serde_json::json!([])),
            updated_at: Set(Some(
                chrono::NaiveDate::from_ymd_opt(2026, 1, 3)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap(),
            )),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        address_book::ActiveModel {
            id: Set("200".to_string()),
            user_id: Set(1),
            alias: Set("   ".to_string()),
            tags: Set(serde_json::json!([])),
            updated_at: Set(Some(
                chrono::NaiveDate::from_ymd_opt(2026, 1, 4)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap(),
            )),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let aliases = aliases_by_user_and_ids(&db, 1, &["100".to_string(), "200".to_string()])
            .await
            .unwrap();

        assert_eq!(aliases.get("100").map(String::as_str), Some("new name"));
        assert_eq!(aliases.get("200"), None);
        assert_eq!(aliases.len(), 1);
    }

    #[tokio::test]
    async fn everyone_rule_applies_to_any_group() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let schema = Schema::new(DbBackend::Sqlite);
        db.execute(
            db.get_database_backend()
                .build(&schema.create_table_from_entity(address_book_collection_rule::Entity)),
        )
        .await
        .unwrap();

        address_book_collection_rule::ActiveModel {
            user_id: Set(1),
            collection_id: Set(7),
            rule: Set(address_book_collection_rule::RULE_READ_WRITE),
            r#type: Set(address_book_collection_rule::RULE_TYPE_GROUP),
            to_id: Set(0),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        assert!(can_read(&db, 42, 99, 1, 7).await.unwrap());
        assert!(can_write(&db, 42, 99, 1, 7).await.unwrap());
        assert!(!can_full_control(&db, 42, 99, 1, 7).await.unwrap());
    }
}
