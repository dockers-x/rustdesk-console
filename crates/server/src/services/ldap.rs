//! LDAP authentication — ports the core of `service/ldap.go` using `ldap3`.
//!
//! Flow: bind as the service account, search the user, check enabled + group
//! membership, verify the password by binding as the user DN, then map/sync to
//! a local user. External integration — requires a real directory to verify.

use ldap3::{LdapConnAsync, LdapConnSettings, Scope, SearchEntry};
use sea_orm::*;

use ::entity::user;

use crate::config::{Config, Ldap};
use crate::services::now;

struct LdapUser {
    dn: String,
    username: String,
    email: String,
    first_name: String,
    last_name: String,
    member_of: Vec<String>,
    enabled: bool,
}

impl LdapUser {
    fn name(&self) -> String {
        format!("{} {}", self.first_name, self.last_name)
            .trim()
            .to_string()
    }
}

fn field_username(c: &Ldap) -> String {
    if c.user.username.is_empty() {
        "uid".into()
    } else {
        c.user.username.clone()
    }
}
fn field_email(c: &Ldap) -> String {
    if c.user.email.is_empty() {
        "mail".into()
    } else {
        c.user.email.clone()
    }
}
fn field_first(c: &Ldap) -> String {
    if c.user.first_name.is_empty() {
        "givenName".into()
    } else {
        c.user.first_name.clone()
    }
}
fn field_last(c: &Ldap) -> String {
    if c.user.last_name.is_empty() {
        "sn".into()
    } else {
        c.user.last_name.clone()
    }
}
fn field_enable(c: &Ldap) -> String {
    if c.user.enable_attr.is_empty() {
        "userAccountControl".into()
    } else {
        c.user.enable_attr.clone()
    }
}
fn base_dn_user(c: &Ldap) -> String {
    if c.user.base_dn.is_empty() {
        c.base_dn.clone()
    } else {
        c.user.base_dn.clone()
    }
}

async fn connect(cfg: &Ldap) -> Result<ldap3::Ldap, String> {
    let settings = LdapConnSettings::new().set_no_tls_verify(!cfg.tls_verify);
    let (conn, ldap) = LdapConnAsync::with_settings(settings, &cfg.url)
        .await
        .map_err(|e| format!("LdapConnectFailed: {e}"))?;
    ldap3::drive!(conn);
    Ok(ldap)
}

async fn connect_bind(cfg: &Ldap, dn: &str, pw: &str) -> Result<ldap3::Ldap, String> {
    let mut ldap = connect(cfg).await?;
    ldap.simple_bind(dn, pw)
        .await
        .map_err(|e| format!("LdapBindFailed: {e}"))?
        .success()
        .map_err(|_| "LdapBindFailed".to_string())?;
    Ok(ldap)
}

async fn search_user(cfg: &Ldap, username: &str) -> Result<Option<LdapUser>, String> {
    let mut ldap = connect_bind(cfg, &cfg.bind_dn, &cfg.bind_password).await?;
    let base = base_dn_user(cfg);
    let filter_cfg = if cfg.user.filter.is_empty() {
        "(cn=*)".to_string()
    } else {
        cfg.user.filter.clone()
    };
    let uattr = field_username(cfg);
    let filter = format!(
        "(&{}({}={}))",
        filter_cfg,
        uattr,
        ldap3::ldap_escape(username)
    );
    let attrs = vec![
        uattr.clone(),
        field_email(cfg),
        field_first(cfg),
        field_last(cfg),
        "memberOf".to_string(),
        field_enable(cfg),
    ];
    let (rs, _res) = ldap
        .search(&base, Scope::Subtree, &filter, attrs)
        .await
        .map_err(|e| format!("LdapSearchRequestFailed: {e}"))?
        .success()
        .map_err(|e| format!("LdapSearchRequestFailed: {e}"))?;
    let _ = ldap.unbind().await;
    if rs.len() != 1 {
        return Ok(None);
    }
    let e = SearchEntry::construct(rs.into_iter().next().unwrap());
    let get = |k: &str| {
        e.attrs
            .get(k)
            .and_then(|v| v.first().cloned())
            .unwrap_or_default()
    };
    let enable_val = get(&field_enable(cfg));
    let mut lu = LdapUser {
        dn: e.dn.clone(),
        username: get(&uattr),
        email: get(&field_email(cfg)),
        first_name: get(&field_first(cfg)),
        last_name: get(&field_last(cfg)),
        member_of: e.attrs.get("memberOf").cloned().unwrap_or_default(),
        enabled: false,
    };
    lu.enabled = is_enabled(cfg, &enable_val);
    Ok(Some(lu))
}

fn is_enabled(cfg: &Ldap, enable_val: &str) -> bool {
    if cfg.user.enable_attr.is_empty() || cfg.user.enable_attr_value.is_empty() {
        return true;
    }
    if cfg.user.enable_attr.to_lowercase() == "useraccountcontrol" {
        match enable_val.parse::<i64>() {
            Ok(v) => v & 0x2 == 0,
            Err(_) => false,
        }
    } else {
        enable_val == cfg.user.enable_attr_value
    }
}

fn in_group(member_of: &[String], group_dn: &str) -> bool {
    member_of.iter().any(|g| g.eq_ignore_ascii_case(group_dn))
}

async fn is_admin(cfg: &Ldap, lu: &LdapUser) -> bool {
    if cfg.user.admin_group.is_empty() {
        return false;
    }
    if !lu.member_of.is_empty() {
        return in_group(&lu.member_of, &cfg.user.admin_group);
    }
    reverse_member_search(cfg, &lu.dn, &cfg.user.admin_group).await
}

/// Reverse `member` search against a group DN (for directories without memberOf).
async fn reverse_member_search(cfg: &Ldap, user_dn: &str, group_dn: &str) -> bool {
    let Ok(mut ldap) = connect_bind(cfg, &cfg.bind_dn, &cfg.bind_password).await else {
        return false;
    };
    let filter = format!("(member={})", ldap3::ldap_escape(user_dn));
    let found = match ldap
        .search(group_dn, Scope::Subtree, &filter, vec!["dn"])
        .await
    {
        Ok(r) => r.success().map(|(rs, _)| !rs.is_empty()).unwrap_or(false),
        Err(_) => false,
    };
    let _ = ldap.unbind().await;
    found
}

/// Authenticate username/password against LDAP and return the local user.
pub async fn authenticate(
    cfg: &Config,
    db: &DatabaseConnection,
    username: &str,
    password: &str,
) -> Result<user::Model, String> {
    let lcfg = &cfg.ldap;
    if !lcfg.enable {
        return Err("LdapNotEnabled".into());
    }
    let lu = search_user(lcfg, username).await?.ok_or("UserNotFound")?;
    if !lu.enabled {
        return Err("UserDisabledAtLdap".into());
    }
    let admin = is_admin(lcfg, &lu).await;
    if !admin && !lcfg.user.allow_group.is_empty() {
        let allowed = if !lu.member_of.is_empty() {
            in_group(&lu.member_of, &lcfg.user.allow_group)
        } else {
            reverse_member_search(lcfg, &lu.dn, &lcfg.user.allow_group).await
        };
        if !allowed {
            return Err("user not in allowed group".into());
        }
    }
    // verify password by binding as the user DN
    let mut verify = connect_bind(lcfg, &lu.dn, password)
        .await
        .map_err(|_| "PasswordNotMatch".to_string())?;
    let _ = verify.unbind().await;

    map_to_local_user(lcfg, db, &lu, admin).await
}

async fn map_to_local_user(
    cfg: &Ldap,
    db: &DatabaseConnection,
    lu: &LdapUser,
    admin: bool,
) -> Result<user::Model, String> {
    let existing = user::Entity::find()
        .filter(user::Column::Username.eq(&lu.username))
        .one(db)
        .await
        .map_err(|e| e.to_string())?;
    let status = if lu.enabled {
        user::STATUS_ENABLE
    } else {
        user::STATUS_DISABLED
    };
    match existing {
        None => {
            let am = user::ActiveModel {
                username: Set(lu.username.clone()),
                email: Set(lu.email.clone()),
                nickname: Set(lu.name()),
                is_admin: Set(Some(admin)),
                group_id: Set(1),
                status: Set(status),
                created_at: Set(now()),
                updated_at: Set(now()),
                ..Default::default()
            };
            am.insert(db)
                .await
                .map_err(|e| format!("LdapCreateUserFailed: {e}"))
        }
        Some(u) => {
            if cfg.user.sync {
                let mut am: user::ActiveModel = u.clone().into();
                am.email = Set(lu.email.clone());
                am.nickname = Set(lu.name());
                am.is_admin = Set(Some(admin));
                am.status = Set(status);
                am.updated_at = Set(now());
                if let Ok(updated) = am.update(db).await {
                    return Ok(updated);
                }
            }
            Ok(u)
        }
    }
}
