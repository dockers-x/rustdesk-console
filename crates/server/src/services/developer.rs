use entity::{developer_key, user};
use rand::{rngs::OsRng, RngCore};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder,
    Set,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Serialize)]
pub struct Scope {
    pub id: &'static str,
    pub label: &'static str,
    pub admin_only: bool,
}

pub const SCOPES: &[Scope] = &[
    Scope {
        id: "profile:read",
        label: "Read own profile",
        admin_only: false,
    },
    Scope {
        id: "devices:read",
        label: "Read devices",
        admin_only: false,
    },
    Scope {
        id: "devices:write",
        label: "Manage devices",
        admin_only: true,
    },
    Scope {
        id: "users:read",
        label: "Read users",
        admin_only: true,
    },
    Scope {
        id: "users:write",
        label: "Manage users",
        admin_only: true,
    },
    Scope {
        id: "device-groups:read",
        label: "Read device groups",
        admin_only: true,
    },
    Scope {
        id: "device-groups:write",
        label: "Create device groups",
        admin_only: true,
    },
    Scope {
        id: "strategies:read",
        label: "Read strategies",
        admin_only: true,
    },
    Scope {
        id: "address-books:read",
        label: "Read own address book",
        admin_only: false,
    },
    Scope {
        id: "address-books:write",
        label: "Manage own address book",
        admin_only: false,
    },
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateKey {
    pub name: String,
    pub scopes: Option<Vec<String>>,
    pub expires_at: Option<i64>,
}

#[derive(Serialize)]
pub struct KeyMetadata {
    pub id: i32,
    pub name: String,
    pub prefix: String,
    pub scopes: Vec<String>,
    pub expires_at: i64,
    pub revoked_at: i64,
    pub created_at: i64,
}

#[derive(Serialize)]
pub struct CreatedKey {
    pub key: String,
    pub metadata: KeyMetadata,
}

#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    #[error("{0}")]
    Invalid(&'static str),
    #[error("Database operation failed")]
    Database(#[from] DbErr),
    #[error("Secure random generation failed")]
    Random,
}

impl TryFrom<developer_key::Model> for KeyMetadata {
    type Error = DbErr;

    fn try_from(row: developer_key::Model) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            name: row.name,
            prefix: row.prefix,
            scopes: serde_json::from_str(&row.scopes)
                .map_err(|_| DbErr::Custom("Invalid API key scopes".into()))?,
            expires_at: row.expires_at,
            revoked_at: row.revoked_at,
            created_at: row.created_at,
        })
    }
}

pub async fn create_key(
    db: &DatabaseConnection,
    owner: &user::Model,
    input: CreateKey,
) -> Result<CreatedKey, KeyError> {
    if !owner.is_enabled() || owner.must_change_password {
        return Err(KeyError::Invalid("Account cannot create API keys"));
    }
    let name = input.name.trim();
    if name.is_empty() || name.len() > 120 {
        return Err(KeyError::Invalid("Name must contain 1 to 120 bytes"));
    }
    let mut scopes = input.scopes.unwrap_or_else(|| {
        SCOPES
            .iter()
            .filter(|scope| scope.id.ends_with(":read") && (!scope.admin_only || owner.is_admin()))
            .map(|scope| scope.id.to_string())
            .collect()
    });
    scopes.sort();
    scopes.dedup();
    if scopes.is_empty()
        || scopes.iter().any(|id| {
            !SCOPES
                .iter()
                .any(|scope| scope.id == id && (!scope.admin_only || owner.is_admin()))
        })
    {
        return Err(KeyError::Invalid("Unknown or unauthorized API scope"));
    }
    let now = chrono::Utc::now().timestamp();
    let expires_at = input.expires_at.unwrap_or(now + 90 * 86400);
    if expires_at <= now || expires_at > now + 365 * 86400 {
        return Err(KeyError::Invalid(
            "Expiry must be future and within 365 days",
        ));
    }
    let mut random = [0_u8; 32];
    OsRng
        .try_fill_bytes(&mut random)
        .map_err(|_| KeyError::Random)?;
    let key = format!("rdc_{}", hex(&random));
    let row = developer_key::ActiveModel {
        owner_id: Set(owner.id),
        token_hash: Set(hash_key(&key)),
        name: Set(name.into()),
        prefix: Set(key[..12].into()),
        scopes: Set(
            serde_json::to_string(&scopes).map_err(|_| KeyError::Invalid("Invalid API scopes"))?
        ),
        expires_at: Set(expires_at),
        revoked_at: Set(0),
        created_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(CreatedKey {
        key,
        metadata: row.try_into()?,
    })
}

pub async fn list_keys(db: &DatabaseConnection, owner_id: i32) -> Result<Vec<KeyMetadata>, DbErr> {
    developer_key::Entity::find()
        .filter(developer_key::Column::OwnerId.eq(owner_id))
        .order_by_desc(developer_key::Column::Id)
        .all(db)
        .await?
        .into_iter()
        .map(TryInto::try_into)
        .collect()
}

pub async fn revoke_key(db: &DatabaseConnection, owner_id: i32, id: i32) -> Result<bool, DbErr> {
    let Some(row) = developer_key::Entity::find_by_id(id)
        .filter(developer_key::Column::OwnerId.eq(owner_id))
        .one(db)
        .await?
    else {
        return Ok(false);
    };
    if row.revoked_at == 0 {
        let mut active: developer_key::ActiveModel = row.into();
        active.revoked_at = Set(chrono::Utc::now().timestamp());
        active.update(db).await?;
    }
    Ok(true)
}

pub struct Principal {
    pub owner: user::Model,
    scopes: Vec<String>,
}

impl Principal {
    pub fn permits(&self, id: &str) -> bool {
        self.scopes.iter().any(|scope| scope == id)
            && SCOPES
                .iter()
                .any(|scope| scope.id == id && (!scope.admin_only || self.owner.is_admin()))
    }
}

pub async fn authenticate(db: &DatabaseConnection, key: &str) -> Result<Option<Principal>, DbErr> {
    if key.len() != 68
        || !key.starts_with("rdc_")
        || !key[4..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Ok(None);
    }
    let Some(row) = developer_key::Entity::find()
        .filter(developer_key::Column::TokenHash.eq(hash_key(key)))
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    if row.revoked_at != 0 || row.expires_at <= chrono::Utc::now().timestamp() {
        return Ok(None);
    }
    let Some(owner) = user::Entity::find_by_id(row.owner_id).one(db).await? else {
        return Ok(None);
    };
    if !owner.is_enabled() || owner.must_change_password {
        return Ok(None);
    }
    let scopes = serde_json::from_str(&row.scopes)
        .map_err(|_| DbErr::Custom("Invalid API key scopes".into()))?;
    Ok(Some(Principal { owner, scopes }))
}

fn hash_key(key: &str) -> String {
    hex(&Sha256::digest(key.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectionTrait, Database, DbBackend, Schema};

    #[tokio::test]
    async fn key_lifecycle_hashes_material_checks_owner_and_revokes() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let schema = Schema::new(DbBackend::Sqlite);
        for statement in [
            schema.create_table_from_entity(developer_key::Entity),
            schema.create_table_from_entity(user::Entity),
        ] {
            db.execute(db.get_database_backend().build(&statement))
                .await
                .unwrap();
        }
        let owner = user::ActiveModel {
            id: Set(1),
            username: Set("owner".into()),
            status: Set(1),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let created = create_key(
            &db,
            &owner,
            CreateKey {
                name: "automation".into(),
                scopes: None,
                expires_at: None,
            },
        )
        .await
        .unwrap();
        assert!(created.key.starts_with("rdc_"));
        let row = developer_key::Entity::find_by_id(created.metadata.id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_ne!(row.token_hash, created.key);
        assert_eq!(row.token_hash, hash_key(&created.key));
        assert!(
            !serde_json::to_string(&list_keys(&db, owner.id).await.unwrap())
                .unwrap()
                .contains(&created.key)
        );
        assert!(authenticate(&db, &created.key).await.unwrap().is_some());
        assert!(!revoke_key(&db, 2, row.id).await.unwrap());
        assert!(revoke_key(&db, owner.id, row.id).await.unwrap());
        assert!(authenticate(&db, &created.key).await.unwrap().is_none());
        let privileged = create_key(
            &db,
            &owner,
            CreateKey {
                name: "forbidden".into(),
                scopes: Some(vec!["users:write".into()]),
                expires_at: None,
            },
        )
        .await;
        assert!(privileged.is_err());
        let expired = create_key(
            &db,
            &owner,
            CreateKey {
                name: "expired".into(),
                scopes: None,
                expires_at: Some(1),
            },
        )
        .await;
        assert!(expired.is_err());
    }
}
