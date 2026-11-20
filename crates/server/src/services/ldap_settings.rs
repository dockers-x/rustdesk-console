use sea_orm::{sea_query::OnConflict, *};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use ::entity::ldap_setting;

use crate::config::Ldap;
use crate::services::ldap;

const SETTINGS_ID: i32 = 1;

#[derive(Default, Deserialize, Serialize)]
struct Overrides {
    settings: Map<String, Value>,
    password: Option<ScopedPassword>,
}

#[derive(Deserialize, Serialize)]
struct ScopedPassword {
    scope: String,
    value: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    #[serde(default)]
    settings: Map<String, Value>,
    // Omission retains the scoped secret; an explicit empty value clears it.
    bind_password: Option<String>,
}

#[derive(Serialize)]
pub struct View {
    settings: Value,
    has_bind_password: bool,
    has_file_ca: bool,
    has_custom_ca: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("LdapSettingsUnavailable")]
    Storage(#[from] DbErr),
    #[error("LdapInvalidSettings")]
    Invalid,
    #[error("LdapInvalidSettingField")]
    InvalidField,
    #[error("LdapSettingsChanged")]
    Concurrent,
    #[error("{0}")]
    Validation(&'static str),
}

async fn load(db: &DatabaseConnection) -> Result<Option<ldap_setting::Model>, DbErr> {
    ldap_setting::Entity::find_by_id(SETTINGS_ID).one(db).await
}

fn decode(row: Option<&ldap_setting::Model>) -> Result<Overrides, SettingsError> {
    row.map(|row| serde_json::from_str(&row.overrides))
        .transpose()
        .map(|value| value.unwrap_or_default())
        .map_err(|_| SettingsError::Invalid)
}

fn public_settings(cfg: &Ldap) -> Value {
    let mut settings = serde_json::to_value(cfg).expect("LDAP settings are serializable");
    if let Some(map) = settings.as_object_mut() {
        map.remove("bind-password");
        map.remove("tls-ca-file");
    }
    settings
}

fn credential_scope(cfg: &Ldap) -> String {
    let fields = (
        &cfg.url,
        &cfg.bind_dn,
        cfg.start_tls,
        cfg.tls_verify,
        if cfg.tls_ca_pem.is_empty() {
            cfg.tls_ca_file.as_str()
        } else {
            ""
        },
        &cfg.tls_ca_pem,
    );
    let bytes = serde_json::to_vec(&fields).expect("LDAP credential scope is serializable");
    format!("{:x}", Sha256::digest(bytes))
}

fn effective(base: &Ldap, overrides: &Overrides) -> Result<Ldap, SettingsError> {
    let mut value = public_settings(base);
    overlay(&mut value, &overrides.settings)?;
    let mut cfg: Ldap = serde_json::from_value(value).map_err(|_| SettingsError::Invalid)?;
    if cfg.tls_ca_pem.is_empty() {
        cfg.tls_ca_file.clone_from(&base.tls_ca_file);
    }
    let scope = credential_scope(&cfg);
    cfg.bind_password = match &overrides.password {
        Some(password) if password.scope == scope => password.value.clone(),
        None if scope == credential_scope(base) => base.bind_password.clone(),
        _ => String::new(),
    };
    Ok(cfg)
}

fn overlay(base: &mut Value, changes: &Map<String, Value>) -> Result<(), SettingsError> {
    let base = base.as_object_mut().ok_or(SettingsError::Invalid)?;
    for (field, value) in changes {
        let dest = base.get_mut(field).ok_or(SettingsError::InvalidField)?;
        match (dest.is_object(), value.as_object()) {
            (true, Some(map)) => overlay(dest, map)?,
            (false, None) => dest.clone_from(value),
            _ => return Err(SettingsError::Invalid),
        }
    }
    Ok(())
}

fn differences(base: &Value, updated: &Value) -> Map<String, Value> {
    let (Some(base), Some(updated)) = (base.as_object(), updated.as_object()) else {
        return Map::new();
    };
    updated
        .iter()
        .filter_map(|(field, value)| {
            let old = base.get(field)?;
            if old == value {
                return None;
            }
            if old.is_object() && value.is_object() {
                return Some((field.clone(), Value::Object(differences(old, value))));
            }
            Some((field.clone(), value.clone()))
        })
        .collect()
}

fn prepare(
    base: &Ldap,
    current: Overrides,
    update: Update,
    test: bool,
) -> Result<(Overrides, Ldap), SettingsError> {
    let original = effective(base, &current)?;
    let mut updated = public_settings(&original);
    overlay(&mut updated, &update.settings)?;
    // Deserialize before computing or using any credential override.
    let updated: Ldap = serde_json::from_value(updated).map_err(|_| SettingsError::Invalid)?;
    let mut overrides = Overrides {
        settings: differences(&public_settings(base), &public_settings(&updated)),
        password: current.password,
    };
    let mut cfg = effective(base, &overrides)?;
    let scope = credential_scope(&cfg);
    if let Some(password) = update.bind_password {
        overrides.password = Some(ScopedPassword {
            scope,
            value: password,
        });
        cfg = effective(base, &overrides)?;
    } else if scope != credential_scope(&original) {
        // Target or trust changed. Never reuse a secret from either configuration source.
        overrides.password = Some(ScopedPassword {
            scope,
            value: String::new(),
        });
        cfg = effective(base, &overrides)?;
    }
    ldap::validate(&cfg, cfg.enable || test).map_err(SettingsError::Validation)?;
    Ok((overrides, cfg))
}

impl From<&Ldap> for View {
    fn from(cfg: &Ldap) -> Self {
        let mut settings = public_settings(cfg);
        if let Some(map) = settings.as_object_mut() {
            map.remove("tls-ca-pem");
            if !cfg.url.is_empty() && !ldap::has_safe_url(cfg) {
                map.insert("url".to_owned(), Value::String(String::new()));
            }
        }
        Self {
            settings,
            has_bind_password: !cfg.bind_password.is_empty(),
            has_file_ca: !cfg.tls_ca_file.is_empty(),
            has_custom_ca: !cfg.tls_ca_pem.is_empty(),
        }
    }
}

pub async fn read(db: &DatabaseConnection, base: &Ldap) -> Result<Ldap, SettingsError> {
    effective(base, &decode(load(db).await?.as_ref())?)
}

pub async fn save(
    db: &DatabaseConnection,
    base: &Ldap,
    update: Update,
) -> Result<View, SettingsError> {
    let previous = load(db).await?;
    let (overrides, cfg) = prepare(base, decode(previous.as_ref())?, update, false)?;
    if previous.is_none() && overrides.settings.is_empty() && overrides.password.is_none() {
        return Ok(View::from(&cfg));
    }
    let encoded = serde_json::to_string(&overrides).map_err(|_| SettingsError::Invalid)?;
    if let Some(row) = previous {
        if encoded == row.overrides {
            return Ok(View::from(&cfg));
        }
        let revision = row
            .revision
            .checked_add(1)
            .ok_or(SettingsError::Concurrent)?;
        let changed = ldap_setting::Entity::update_many()
            .col_expr(
                ldap_setting::Column::Overrides,
                sea_query::Expr::value(encoded),
            )
            .col_expr(
                ldap_setting::Column::Revision,
                sea_query::Expr::value(revision),
            )
            .filter(ldap_setting::Column::Id.eq(SETTINGS_ID))
            .filter(ldap_setting::Column::Revision.eq(row.revision))
            .exec(db)
            .await?;
        if changed.rows_affected != 1 {
            return Err(SettingsError::Concurrent);
        }
    } else {
        let result = ldap_setting::Entity::insert(ldap_setting::ActiveModel {
            id: Set(SETTINGS_ID),
            revision: Set(1),
            overrides: Set(encoded),
        })
        .on_conflict(
            OnConflict::column(ldap_setting::Column::Id)
                .do_nothing()
                .to_owned(),
        )
        .exec_without_returning(db)
        .await?;
        if result != 1 {
            return Err(SettingsError::Concurrent);
        }
    }
    Ok(View::from(&cfg))
}

pub async fn test(
    db: &DatabaseConnection,
    base: &Ldap,
    update: Update,
) -> Result<(), SettingsError> {
    let (_, cfg) = prepare(base, decode(load(db).await?.as_ref())?, update, true)?;
    ldap::test_bind(&cfg)
        .await
        .map_err(SettingsError::Validation)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{bootstrap, config::Config, i18n::I18n};
    use serde_json::json;

    pub(crate) async fn test_db() -> DatabaseConnection {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        bootstrap::migrate_and_seed(&db, &Config::default(), &I18n::load("en"), "en")
            .await
            .unwrap();
        db
    }

    fn update(value: Value) -> Update {
        serde_json::from_value(value).unwrap()
    }

    fn file_config() -> Ldap {
        Ldap {
            enable: true,
            url: "ldap://file.example.test:389".to_owned(),
            base_dn: "dc=fixture".to_owned(),
            bind_dn: "cn=reader,dc=fixture".to_owned(),
            bind_password: "file-secret".to_owned(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn ldap_inherits_file_configuration_and_persists_only_overrides() {
        let db = test_db().await;
        let mut file = file_config();
        let config = read(&db, &file).await.unwrap();
        assert_eq!(config.bind_password, file.bind_password);
        assert_eq!(config.url, file.url);
        save(
            &db,
            &file,
            update(json!({"settings":{"user":{"scope":"one"}}})),
        )
        .await
        .unwrap();
        let stored = decode(load(&db).await.unwrap().as_ref()).unwrap();
        assert_eq!(
            Value::Object(stored.settings),
            json!({"user":{"scope":"one"}})
        );
        assert!(stored.password.is_none());

        file.bind_password = "rotated-file-secret".to_owned();
        file.user.email = "secondaryMail".to_owned();
        let config = read(&db, &file).await.unwrap();
        assert_eq!(config.bind_password, file.bind_password);
        assert_eq!(config.user.email, file.user.email);
        assert!(matches!(config.user.scope, crate::config::LdapScope::One));
    }

    #[tokio::test]
    async fn ldap_scopes_secrets_to_the_target_and_redacts_views() {
        let db = test_db().await;
        let mut file = file_config();
        let invalid = save(
            &db,
            &file,
            update(json!({"settings":{"url":"ldaps://another.example.test"}})),
        )
        .await;
        assert!(matches!(
            invalid,
            Err(SettingsError::Validation("LdapInvalidBindCredentials"))
        ));
        assert!(load(&db).await.unwrap().is_none());

        let response = save(
            &db,
            &file,
            update(json!({
                "settings":{"url":"ldaps://another.example.test","tls-verify":true},
                "bind_password":"web-secret"
            })),
        )
        .await
        .unwrap();
        let response = serde_json::to_value(response).unwrap();
        assert_eq!(response["has_bind_password"], true);
        assert!(!response.to_string().contains("web-secret"));
        assert!(!response.to_string().contains("bind-password"));
        assert!(!format!("{file:?}").contains("file-secret"));
        assert_eq!(read(&db, &file).await.unwrap().bind_password, "web-secret");

        file.bind_dn = "cn=another-reader,dc=fixture".to_owned();
        file.bind_password = "new-file-secret".to_owned();
        assert!(read(&db, &file).await.unwrap().bind_password.is_empty());
        save(
            &db,
            &file,
            update(json!({"settings":{"enable":false},"bind_password":""})),
        )
        .await
        .unwrap();
        assert!(read(&db, &file).await.unwrap().bind_password.is_empty());
    }

    #[test]
    fn ldap_rejects_invalid_scope_secret_fields_and_incompatible_tls() {
        let file = file_config();
        for invalid in [
            json!({"settings":{"user":{"scope":"whole"}}}),
            json!({"settings":{"bind-password":"nested-secret"}}),
            json!({"settings":{"url":"ldap://reader:secret@ldap.example.test/"}}),
            json!({"settings":{"url":"ldaps://ldap.example.test","start-tls":true}}),
            json!({"settings":{"tls-ca-pem":"invalid"}}),
        ] {
            assert!(prepare(&file, Overrides::default(), update(invalid), false).is_err());
        }
        let unsafe_file = Ldap {
            url: "ldap://reader:secret@ldap.example.test/".to_owned(),
            ..file
        };
        let view = serde_json::to_string(&View::from(&unsafe_file)).unwrap();
        assert!(!view.contains("secret"));
    }
}
