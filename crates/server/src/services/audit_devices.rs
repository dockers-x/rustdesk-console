//! Resolve the device IDs displayed on administrative audit and recording lists.

use std::collections::{HashMap, HashSet};

use entity::{address_book, peer};
use sea_orm::{
    sea_query::{Alias, Expr, Func, Query},
    ColumnTrait, ConnectionTrait, DatabaseBackend, DatabaseConnection, EntityTrait, QueryFilter,
    QuerySelect,
};
use serde::Serialize;

#[derive(Clone, Serialize)]
pub(crate) struct DeviceReference {
    target: Target,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum Target {
    AddressBook,
    Device,
}

fn reference(target: Target, alias: String, hostname: String, note: String) -> DeviceReference {
    DeviceReference {
        target,
        name: [alias, hostname]
            .into_iter()
            .find(|name| !name.trim().is_empty()),
        note: Some(note).filter(|note| !note.trim().is_empty()),
    }
}

pub(crate) async fn lookup<'a>(
    db: &DatabaseConnection,
    allowed: bool,
    ids: impl IntoIterator<Item = &'a str>,
) -> HashMap<String, DeviceReference> {
    if !allowed {
        return HashMap::new();
    }
    // Bound the SQL parameter count even when an API caller requests a large page.
    let mut seen = HashSet::new();
    let requested: Vec<_> = ids
        .into_iter()
        .filter(|id| !id.trim().is_empty() && id.len() <= 128 && seen.insert(*id))
        .take(200)
        .collect();
    if requested.is_empty() {
        return HashMap::new();
    }

    // Choose one address-book row for each ID so the name and note describe
    // the same entry even when that ID appears in several collections.
    use address_book::Column as BookColumn;
    let informative_rows =
        [BookColumn::Alias, BookColumn::Note, BookColumn::Hostname].map(|column| {
            Func::min(Expr::case(
                Expr::expr(Func::cust(Alias::new("TRIM")).arg(Expr::col(column))).ne(""),
                Expr::col(BookColumn::RowId),
            ))
            .into()
        });
    let backend = db.get_database_backend();
    let book_id = super::exact_device_id(backend, BookColumn::Id);
    let selected_rows = Query::select()
        .expr(Func::coalesce(
            informative_rows
                .into_iter()
                .chain([Func::min(Expr::col(BookColumn::RowId)).into()]),
        ))
        .from(address_book::Entity)
        .and_where_option(
            (backend == DatabaseBackend::MySql)
                .then(|| BookColumn::Id.is_in(requested.iter().copied())),
        )
        .and_where(book_id.clone().is_in(requested.iter().copied()))
        .add_group_by([book_id.into()])
        .to_owned();
    let books = address_book::Entity::find()
        .select_only()
        .column(BookColumn::Id)
        .column(BookColumn::Alias)
        .column(BookColumn::Hostname)
        .column(BookColumn::Note)
        .filter(BookColumn::RowId.in_subquery(selected_rows))
        .into_tuple::<(String, String, String, String)>()
        .all(db)
        .await
        .unwrap_or_default();
    let mut references: HashMap<_, _> = books
        .into_iter()
        .filter(|(id, _, _, _)| seen.contains(id.as_str()))
        .map(|(id, alias, hostname, note)| {
            (id, reference(Target::AddressBook, alias, hostname, note))
        })
        .collect();

    let missing = requested
        .into_iter()
        .filter(|id| !references.contains_key(*id))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return references;
    }
    use peer::Column as PeerColumn;
    let peer_id = super::exact_device_id(backend, PeerColumn::Id);
    let selected_rows = Query::select()
        .expr(Func::coalesce([
            Func::min(Expr::case(
                Expr::expr(Func::cust(Alias::new("TRIM")).arg(Expr::col(PeerColumn::Alias))).ne(""),
                Expr::col(PeerColumn::RowId),
            ))
            .into(),
            Func::min(Expr::col(PeerColumn::RowId)).into(),
        ]))
        .from(peer::Entity)
        .and_where_option(
            (backend == DatabaseBackend::MySql)
                .then(|| PeerColumn::Id.is_in(missing.iter().copied())),
        )
        .and_where(peer_id.clone().is_in(missing))
        .add_group_by([peer_id.into()])
        .to_owned();
    let devices = peer::Entity::find()
        .select_only()
        .column(PeerColumn::Id)
        .column(PeerColumn::Alias)
        .column(PeerColumn::Hostname)
        .filter(PeerColumn::RowId.in_subquery(selected_rows))
        .into_tuple::<(String, String, String)>()
        .all(db)
        .await
        .unwrap_or_default();
    references.extend(
        devices
            .into_iter()
            .filter(|(id, _, _)| seen.contains(id.as_str()))
            .map(|(id, alias, hostname)| {
                (
                    id,
                    reference(Target::Device, alias, hostname, String::new()),
                )
            }),
    );
    references
}

#[derive(Serialize)]
pub(crate) struct DeviceFields<T> {
    #[serde(flatten)]
    row: T,
    #[serde(skip_serializing_if = "Option::is_none")]
    peer_id_device: Option<DeviceReference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    from_peer_device: Option<DeviceReference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    device_id_device: Option<DeviceReference>,
}

impl<T> DeviceFields<T> {
    pub(crate) fn peers(
        row: T,
        peer_id_device: Option<DeviceReference>,
        from_peer_device: Option<DeviceReference>,
    ) -> Self {
        Self {
            row,
            peer_id_device,
            from_peer_device,
            device_id_device: None,
        }
    }

    pub(crate) fn login(row: T, device_id_device: Option<DeviceReference>) -> Self {
        Self {
            row,
            peer_id_device: None,
            from_peer_device: None,
            device_id_device,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ActiveModelTrait, ConnectOptions, Database, Schema, Set};
    use serde_json::json;

    #[tokio::test]
    async fn looks_up_visible_names_exactly_and_preserves_raw_ids() {
        check_exact_device_lookup("sqlite::memory:").await;
    }

    #[tokio::test]
    #[ignore = "requires a local MariaDB fixture"]
    async fn exact_device_ids_survive_case_insensitive_mariadb_collation() {
        let url = std::env::var("RUSTDESK_AUDIT_DEVICES_TEST_DATABASE_URL").unwrap();
        check_exact_device_lookup(&url).await;
    }

    async fn check_exact_device_lookup(url: &str) {
        let mut options = ConnectOptions::new(url);
        options.max_connections(1);
        let db = Database::connect(options).await.unwrap();
        assert!(lookup(&db, false, ["audit-only"]).await.is_empty());
        let backend = db.get_database_backend();
        let schema = Schema::new(backend);
        for mut table in [
            schema.create_table_from_entity(address_book::Entity),
            schema.create_table_from_entity(peer::Entity),
        ] {
            table.temporary();
            if backend == DatabaseBackend::MySql {
                table.collate("utf8mb4_unicode_ci");
            }
            db.execute(backend.build(&table)).await.unwrap();
        }
        for (id, alias, note) in [
            ("r-d/1", "", "Z different collection note"),
            ("r-d/1", "Shared alias", "Private device note"),
            ("r-d/1", "Z later alias", "Z latest note"),
            ("r-d/11", "Unrelated alias", "Unrelated note"),
            ("R-D/1", "Different case", "Different case note"),
        ] {
            address_book::ActiveModel {
                id: Set(id.to_owned()),
                alias: Set(alias.to_owned()),
                tags: Set(json!([])),
                note: Set(note.to_owned()),
                password: Set("password-never-projected".to_owned()),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }
        for (id, alias, hostname) in [
            ("r-d/1", "Peer alias", "Peer host"),
            ("second", "", "Second hostname"),
            ("Second", "", "Capitalized hostname"),
        ] {
            peer::ActiveModel {
                id: Set(id.to_owned()),
                alias: Set(alias.to_owned()),
                hostname: Set(hostname.to_owned()),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }
        let ids = ["r-d/1", "r-d/1", "R-D/1", "second", "Second", "unknown", ""];
        let visible = lookup(&db, true, ids).await;
        let values = serde_json::to_value(&visible).unwrap();
        assert_eq!(
            values,
            json!({
                "r-d/1": {"target": "address_book", "name": "Shared alias", "note": "Private device note"},
                "R-D/1": {"target": "address_book", "name": "Different case", "note": "Different case note"},
                "second": {"target": "device", "name": "Second hostname"},
                "Second": {"target": "device", "name": "Capitalized hostname"}
            })
        );
        assert!(lookup(&db, false, ids).await.is_empty());

        let exact = crate::services::address_book::admin_list(
            &db,
            1,
            10,
            crate::services::address_book::AbFilters {
                id: Some("r-d/1".to_owned()),
                exact_id: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(exact.total, 3);
        assert!(exact.list.iter().all(|row| row.id == "r-d/1"));

        let exact = crate::services::peer::list_filtered(
            &db,
            1,
            10,
            crate::services::peer::PeerFilters {
                id: Some("Second".to_owned()),
                exact_id: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(exact.total, 1);
        assert_eq!(exact.list[0].id, "Second");
    }
}
