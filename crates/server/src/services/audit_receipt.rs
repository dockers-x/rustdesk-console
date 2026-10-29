use entity::audit_receipt;
use sea_orm::{
    ActiveModelTrait, DatabaseConnection, DatabaseTransaction, DbErr, Set, SqlErr, TransactionTrait,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug)]
pub(crate) enum Kind {
    Connection,
    File,
    Alarm,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Connection => "conn",
            Self::File => "file",
            Self::Alarm => "alarm",
        }
    }
}

pub(crate) fn has_nonce(body: &Value) -> bool {
    body["nonce"]
        .as_str()
        .is_some_and(|nonce| !nonce.is_empty())
}

pub(crate) async fn record_event(
    db: &DatabaseConnection,
    kind: Kind,
    body: &Value,
) -> Result<(), DbErr> {
    let Some(transaction) = begin(db, kind.as_str(), body).await? else {
        return Ok(());
    };
    match kind {
        Kind::Connection => super::audit::record_conn_event(&transaction, body).await?,
        Kind::File => super::audit::record_file_event(&transaction, body).await?,
        Kind::Alarm => super::audit_alarm::record_event(&transaction, body).await?,
    }
    transaction.commit().await
}

async fn begin(
    db: &DatabaseConnection,
    kind: &str,
    body: &Value,
) -> Result<Option<DatabaseTransaction>, DbErr> {
    let nonce = body["nonce"]
        .as_str()
        .filter(|nonce| !nonce.is_empty())
        .ok_or_else(|| DbErr::Custom("missing audit nonce".to_owned()))?;
    let identity = serde_json::to_vec(&[
        kind,
        body["id"].as_str().unwrap_or_default(),
        body["uuid"].as_str().unwrap_or_default(),
        nonce,
    ])
    .map_err(|error| DbErr::Custom(error.to_string()))?;
    let transaction = db.begin().await?;
    let receipt = audit_receipt::ActiveModel {
        id: Set(format!("{:x}", Sha256::digest(identity))),
        kind: Set(kind.to_owned()),
        payload: Set(body.clone()),
        created_at: Set(super::now()),
    };
    match receipt.insert(&transaction).await {
        Ok(_) => Ok(Some(transaction)),
        Err(error) if matches!(error.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) => {
            transaction.rollback().await?;
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use entity::{audit_conn, audit_file};
    use sea_orm::{ConnectOptions, ConnectionTrait, Database, EntityTrait, PaginatorTrait, Schema};
    use serde_json::json;

    async fn database() -> DatabaseConnection {
        let mut options = ConnectOptions::new("sqlite::memory:");
        options.max_connections(1);
        let db = Database::connect(options).await.unwrap();
        let schema = Schema::new(db.get_database_backend());
        for statement in [
            schema.create_table_from_entity(audit_receipt::Entity),
            schema.create_table_from_entity(audit_conn::Entity),
            schema.create_table_from_entity(audit_file::Entity),
        ] {
            db.execute(db.get_database_backend().build(&statement))
                .await
                .unwrap();
        }
        db
    }

    #[tokio::test]
    async fn retries_create_one_event_and_keep_association_metadata() {
        let db = database().await;
        let body = json!({"id":"123", "uuid":"device", "nonce":"event", "action":"new", "conn_audit_ref":"reference"});
        for _ in 0..2 {
            record_event(&db, Kind::Connection, &body).await.unwrap();
            record_event(&db, Kind::File, &body).await.unwrap();
        }
        assert_eq!(audit_conn::Entity::find().count(&db).await.unwrap(), 1);
        assert_eq!(audit_file::Entity::find().count(&db).await.unwrap(), 1);
        let receipt = audit_receipt::Entity::find()
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(receipt.payload["conn_audit_ref"], "reference");
    }

    #[tokio::test]
    async fn failed_event_can_be_retried_without_losing_it() {
        let db = database().await;
        db.execute_unprepared("DROP TABLE audit_files")
            .await
            .unwrap();
        let body = json!({"id":"123", "uuid":"device", "nonce":"event"});
        assert!(record_event(&db, Kind::File, &body).await.is_err());
        let statement =
            Schema::new(db.get_database_backend()).create_table_from_entity(audit_file::Entity);
        db.execute(db.get_database_backend().build(&statement))
            .await
            .unwrap();
        record_event(&db, Kind::File, &body).await.unwrap();
        assert_eq!(audit_file::Entity::find().count(&db).await.unwrap(), 1);
    }
}
