//! Bounded administration of assigned user groups and shared address books.

use entity::{
    address_book, address_book_collection as collection, address_book_collection_rule as rule,
    group, system_setting, user,
};
use sea_orm::{
    sea_query::{Alias, BinOper, Expr, Func, Query, SimpleExpr},
    ActiveModelTrait, ColumnTrait, Condition, ConnectionTrait, DatabaseConnection,
    DatabaseTransaction, DbBackend, DbErr, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect, Select, Set,
};
use serde::{Deserialize, Serialize};

use super::{
    console_roles::{self, ConsoleRole},
    now, paginate,
};

pub(crate) enum Error {
    Denied,
    Invalid,
    Database,
}

impl From<DbErr> for Error {
    fn from(error: DbErr) -> Self {
        tracing::warn!(%error, "Delegated administration database operation failed");
        Self::Database
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Actor {
    pub user_id: i32,
    pub session_id: i32,
}

struct Scope {
    groups: Vec<i32>,
    collections: Vec<i32>,
}

async fn scope(db: &impl ConnectionTrait, actor: Actor) -> Result<Scope, Error> {
    if let ConsoleRole::ScopedAdministrator {
        group_ids,
        collection_ids,
    } = console_roles::for_user(db, actor.user_id).await?
    {
        Ok(Scope {
            groups: group_ids,
            collections: collection_ids,
        })
    } else {
        Err(Error::Denied)
    }
}

async fn write_scope(
    db: &DatabaseConnection,
    actor: Actor,
) -> Result<(DatabaseTransaction, Scope), Error> {
    let transaction = console_roles::authority_transaction(db).await?;
    let account = super::user::lock_account(&transaction, actor.user_id)
        .await?
        .filter(|account| !account.is_admin())
        .ok_or(Error::Denied)?;
    super::user::lock_full_session(&transaction, &account, actor.session_id)
        .await
        .map_err(|_| Error::Denied)?;
    let scope = scope(&transaction, actor).await?;
    Ok((transaction, scope))
}

pub(super) fn shared_collections() -> Select<collection::Entity> {
    let recipient = |column| Expr::col((rule::Entity, column));
    let existing_user = Query::select()
        .column(user::Column::Id)
        .from(user::Entity)
        .and_where(
            Expr::col((user::Entity, user::Column::Id)).equals((rule::Entity, rule::Column::ToId)),
        )
        .to_owned();
    let existing_group = Query::select()
        .column(group::Column::Id)
        .from(group::Entity)
        .and_where(
            Expr::col((group::Entity, group::Column::Id))
                .equals((rule::Entity, rule::Column::ToId)),
        )
        .to_owned();
    let share_target = Condition::any()
        .add(
            Condition::all()
                .add(rule::Column::Type.eq(rule::RULE_TYPE_PERSONAL))
                .add(recipient(rule::Column::ToId).ne(recipient(rule::Column::UserId)))
                .add(Expr::exists(existing_user)),
        )
        .add(
            Condition::all()
                .add(rule::Column::Type.eq(rule::RULE_TYPE_GROUP))
                .add(
                    Condition::any()
                        .add(rule::Column::ToId.eq(0))
                        .add(Expr::exists(existing_group)),
                ),
        );
    let valid_sharing_rule = Query::select()
        .column(rule::Column::Id)
        .from(rule::Entity)
        .and_where(
            Expr::col((rule::Entity, rule::Column::CollectionId))
                .equals((collection::Entity, collection::Column::Id)),
        )
        .and_where(
            Expr::col((rule::Entity, rule::Column::UserId))
                .equals((collection::Entity, collection::Column::UserId)),
        )
        .cond_where(share_target)
        .and_where(rule::Column::Rule.between(rule::RULE_READ, rule::RULE_FULL_CONTROL))
        .to_owned();

    let existing_owner = Query::select()
        .column(user::Column::Id)
        .from(user::Entity)
        .and_where(
            Expr::col((user::Entity, user::Column::Id))
                .equals((collection::Entity, collection::Column::UserId)),
        )
        .to_owned();
    collection::Entity::find()
        .filter(collection::Column::Id.gt(0))
        .filter(Expr::exists(valid_sharing_rule))
        .filter(Expr::exists(existing_owner))
}

#[derive(Serialize)]
pub(crate) struct ScopeView {
    groups: Vec<group::Model>,
    collections: Vec<collection::Model>,
}

pub(crate) async fn scope_view(db: &DatabaseConnection, actor: Actor) -> Result<ScopeView, Error> {
    let scope = scope(db, actor).await?;
    let groups = group::Entity::find()
        .filter(group::Column::Id.is_in(scope.groups))
        .order_by_asc(group::Column::Name)
        .order_by_asc(group::Column::Id)
        .all(db)
        .await?;
    let collections = shared_collections()
        .filter(collection::Column::Id.is_in(scope.collections))
        .order_by_asc(collection::Column::Name)
        .order_by_asc(collection::Column::Id)
        .all(db)
        .await?;
    Ok(ScopeView {
        groups,
        collections,
    })
}

#[derive(Deserialize)]
pub(crate) struct PageQuery {
    #[serde(default)]
    page: u64,
    #[serde(default)]
    page_size: u64,
}

#[derive(Serialize)]
pub(crate) struct Page<T> {
    list: Vec<T>,
    total: u64,
}

pub(crate) async fn collection_options(
    db: &DatabaseConnection,
    page: PageQuery,
) -> Result<Page<collection::Model>, Error> {
    let (page, size) = paginate(page.page, page.page_size);
    let query = shared_collections().order_by_asc(collection::Column::Id);
    let total = query.clone().count(db).await?;
    let list = query.paginate(db, size).fetch_page(page - 1).await?;
    Ok(Page { list, total })
}

async fn scoped_group(
    db: &impl ConnectionTrait,
    scope: &Scope,
    group_id: i32,
) -> Result<(), Error> {
    if scope.groups.contains(&group_id)
        && group::Entity::find_by_id(group_id).one(db).await?.is_some()
    {
        Ok(())
    } else {
        Err(Error::Denied)
    }
}

async fn scoped_collection(
    db: &impl ConnectionTrait,
    scope: &Scope,
    id: i32,
) -> Result<collection::Model, Error> {
    if !scope.collections.contains(&id) {
        return Err(Error::Denied);
    }
    shared_collections()
        .filter(collection::Column::Id.eq(id))
        .one(db)
        .await?
        .ok_or(Error::Denied)
}

fn ordinary_group_users(group_id: i32, backend: DbBackend) -> Select<user::Entity> {
    let user_id = Expr::col((user::Entity, user::Column::Id));
    let role_key: SimpleExpr = if backend == DbBackend::MySql {
        Func::cust(Alias::new("CONCAT"))
            .args([Expr::value(console_roles::KEY_PREFIX), user_id.into()])
            .into()
    } else {
        Expr::val(console_roles::KEY_PREFIX)
            .binary(BinOper::Custom("||"), user_id.cast_as(Alias::new("TEXT")))
    };
    let privileged = Query::select()
        .column(system_setting::Column::Id)
        .from(system_setting::Entity)
        .and_where(Expr::col((system_setting::Entity, system_setting::Column::Key)).eq(role_key))
        .to_owned();
    user::Entity::find()
        .filter(user::Column::GroupId.eq(group_id))
        .filter(
            Condition::any()
                .add(user::Column::IsAdmin.eq(false))
                .add(user::Column::IsAdmin.is_null()),
        )
        .filter(Expr::exists(privileged).not())
}

#[derive(Serialize)]
pub(crate) struct UserSummary {
    id: i32,
    username: String,
    nickname: String,
    status: i32,
}

pub(crate) async fn group_users(
    db: &DatabaseConnection,
    actor: Actor,
    group_id: i32,
    page: PageQuery,
) -> Result<Page<UserSummary>, Error> {
    let scope = scope(db, actor).await?;
    scoped_group(db, &scope, group_id).await?;
    let (page, size) = paginate(page.page, page.page_size);
    let query =
        ordinary_group_users(group_id, db.get_database_backend()).order_by_asc(user::Column::Id);
    let total = query.clone().count(db).await?;
    let list = query
        .paginate(db, size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(|account| UserSummary {
            id: account.id,
            username: account.username,
            nickname: account.nickname,
            status: account.status,
        })
        .collect();
    Ok(Page { list, total })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UserStatus {
    user_ids: Vec<i32>,
    enabled: bool,
}

pub(crate) async fn set_user_status(
    db: &DatabaseConnection,
    actor: Actor,
    group_id: i32,
    input: UserStatus,
) -> Result<(), Error> {
    let mut ids = input.user_ids;
    if ids.is_empty() || ids.len() > 50 || ids.iter().any(|id| *id <= 0) {
        return Err(Error::Invalid);
    }
    ids.sort_unstable();
    let original_count = ids.len();
    ids.dedup();
    if ids.len() != original_count {
        return Err(Error::Invalid);
    }
    let (transaction, scope) = write_scope(db, actor).await?;
    scoped_group(&transaction, &scope, group_id).await?;
    let users = ordinary_group_users(group_id, db.get_database_backend())
        .filter(user::Column::Id.is_in(ids.iter().copied()))
        .order_by_asc(user::Column::Id)
        .lock_exclusive()
        .all(&transaction)
        .await?;
    if users.len() != ids.len() {
        return Err(Error::Denied);
    }
    let status = if input.enabled {
        user::STATUS_ENABLE
    } else {
        user::STATUS_DISABLED
    };
    for account in users {
        if account.status != status {
            let id = account.id;
            let mut update: user::ActiveModel = account.into();
            update.status = Set(status);
            update.updated_at = Set(now());
            update.update(&transaction).await?;
            // Other administration paths may leave session rows for disabled
            // accounts. Enabling an account must not reactivate those sessions.
            super::user::revoke_credentials(&transaction, id).await?;
        }
    }
    transaction.commit().await?;
    Ok(())
}

#[derive(Serialize)]
pub(crate) struct Entry {
    row_id: i32,
    id: String,
    alias: String,
    note: String,
}

fn entries(collection: &collection::Model) -> Select<address_book::Entity> {
    address_book::Entity::find()
        .filter(address_book::Column::CollectionId.eq(collection.id))
        .filter(address_book::Column::UserId.eq(collection.user_id))
}

pub(crate) async fn collection_entries(
    db: &DatabaseConnection,
    actor: Actor,
    id: i32,
    page: PageQuery,
) -> Result<Page<Entry>, Error> {
    let scope = scope(db, actor).await?;
    let collection = scoped_collection(db, &scope, id).await?;
    let (page, size) = paginate(page.page, page.page_size);
    let query = entries(&collection).order_by_asc(address_book::Column::RowId);
    let total = query.clone().count(db).await?;
    let list = query
        .paginate(db, size)
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(|entry| Entry {
            row_id: entry.row_id,
            id: entry.id,
            alias: entry.alias,
            note: entry.note,
        })
        .collect();
    Ok(Page { list, total })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CollectionProfile {
    name: String,
    note: String,
}

pub(crate) async fn update_collection(
    db: &DatabaseConnection,
    actor: Actor,
    id: i32,
    input: CollectionProfile,
) -> Result<(), Error> {
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > 255 {
        return Err(Error::Invalid);
    }
    let (transaction, scope) = write_scope(db, actor).await?;
    let collection = scoped_collection(&transaction, &scope, id).await?;
    if input.note != collection.note && input.note.chars().count() > 255 {
        return Err(Error::Invalid);
    }
    let duplicate = collection::Entity::find()
        .filter(collection::Column::UserId.eq(collection.user_id))
        .filter(collection::Column::Name.eq(name))
        .filter(collection::Column::Id.ne(id))
        .one(&transaction)
        .await?
        .is_some();
    if duplicate {
        return Err(Error::Invalid);
    }
    let mut update: collection::ActiveModel = collection.into();
    update.name = Set(name.to_owned());
    update.note = Set(input.note);
    update.updated_at = Set(now());
    update.update(&transaction).await?;
    transaction.commit().await?;
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EntryProfile {
    alias: String,
    note: String,
}

pub(crate) async fn update_entry(
    db: &DatabaseConnection,
    actor: Actor,
    collection_id: i32,
    row_id: i32,
    input: EntryProfile,
) -> Result<(), Error> {
    if input.alias.chars().count() > 255 {
        return Err(Error::Invalid);
    }
    let (transaction, scope) = write_scope(db, actor).await?;
    let collection = scoped_collection(&transaction, &scope, collection_id).await?;
    let entry = entries(&collection)
        .filter(address_book::Column::RowId.eq(row_id))
        .lock_exclusive()
        .one(&transaction)
        .await?
        .ok_or(Error::Denied)?;
    if input.note != entry.note && input.note.chars().count() > 255 {
        return Err(Error::Invalid);
    }
    let mut update: address_book::ActiveModel = entry.into();
    update.alias = Set(input.alias);
    update.note = Set(input.note);
    update.updated_at = Set(now());
    update.update(&transaction).await?;
    transaction.commit().await?;
    Ok(())
}
