//! Canonical blocklist rules and reports from paired Server processes.

use std::{
    net::IpAddr,
    time::{Duration, Instant},
};

use entity::{blocklist_rule, peer};
use ipnetwork::{IpNetwork, Ipv4Network};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, ConnectionTrait, DatabaseConnection, DbErr,
    EntityTrait, FromQueryResult, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use super::{console_audit, console_roles};

const MAX_RULES: u64 = 1024;
const MAX_DEVICE_ID_BYTES: usize = 128;
const PAIRING_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Kind {
    Device,
    Ip,
    Cidr,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Device => "device",
            Self::Ip => "ip",
            Self::Cidr => "cidr",
        }
    }

    fn canonical_value(self, raw: &str) -> Result<String, Error> {
        let raw = raw.trim();
        match self {
            Self::Device
                if !raw.is_empty()
                    && raw.len() <= MAX_DEVICE_ID_BYTES
                    && raw.chars().all(|c| !c.is_control() && !c.is_whitespace()) =>
            {
                Ok(raw.into())
            }
            Self::Device => Err(Error::Invalid),
            Self::Ip => raw
                .parse::<IpAddr>()
                .map(|ip| ip.to_canonical().to_string())
                .map_err(|_| Error::Invalid),
            Self::Cidr => {
                if !raw.contains('/') {
                    return Err(Error::Invalid);
                }
                let network = raw.parse::<IpNetwork>().map_err(|_| Error::Invalid)?;
                let normalized = IpNetwork::new(network.network(), network.prefix())
                    .map_err(|_| Error::Invalid)?;
                if let IpNetwork::V6(network) = normalized {
                    if let Some(ip) = network.network().to_ipv4_mapped() {
                        return Ipv4Network::new(ip, network.prefix() - 96)
                            .map(|network| network.to_string())
                            .map_err(|_| Error::Invalid);
                    }
                }
                Ok(normalized.to_string())
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Form {
    kind: Kind,
    value: String,
    #[serde(default)]
    note: String,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("invalid blocklist rule")]
    Invalid,
    #[error("the blocklist rule already exists")]
    Duplicate,
    #[error("the blocklist is full")]
    Full,
    #[error("blocklist rule not found")]
    NotFound,
    #[error(transparent)]
    Database(#[from] DbErr),
}

pub(crate) async fn rules(db: &impl ConnectionTrait) -> Result<Vec<blocklist_rule::Model>, DbErr> {
    blocklist_rule::Entity::find()
        .order_by_asc(blocklist_rule::Column::Id)
        .limit(MAX_RULES + 1)
        .all(db)
        .await
}

pub(crate) async fn blocks_guest(
    db: &DatabaseConnection,
    peer_id: &str,
    source: Option<IpAddr>,
) -> Result<bool, DbErr> {
    let source = source.map(|ip| ip.to_canonical());
    Ok(rules(db)
        .await?
        .iter()
        .any(|rule| match rule.kind.as_str() {
            "device" => rule.value == peer_id,
            "ip" => source.is_some_and(|ip| rule.value.parse::<IpAddr>() == Ok(ip)),
            "cidr" => source.is_some_and(|ip| {
                rule.value
                    .parse::<IpNetwork>()
                    .is_ok_and(|network| network.contains(ip))
            }),
            _ => false,
        }))
}

pub(crate) async fn save(
    db: &DatabaseConnection,
    actor_id: i32,
    id: Option<i32>,
    form: Form,
) -> Result<blocklist_rule::Model, Error> {
    let kind = form.kind.as_str();
    let value = form.kind.canonical_value(&form.value)?;
    let note = form.note.trim();
    if note.chars().count() > 255 || note.chars().any(char::is_control) {
        return Err(Error::Invalid);
    }
    let rule_key = format!("{:x}", Sha256::digest(format!("{kind}\0{value}")));
    let transaction = console_roles::authority_transaction(db).await?;
    let existing = blocklist_rule::Entity::find()
        .filter(blocklist_rule::Column::RuleKey.eq(&rule_key))
        .one(&transaction)
        .await?;
    if existing.is_some_and(|existing| Some(existing.id) != id) {
        return Err(Error::Duplicate);
    }
    let mut active: blocklist_rule::ActiveModel = match id {
        Some(id) => blocklist_rule::Entity::find_by_id(id)
            .one(&transaction)
            .await?
            .ok_or(Error::NotFound)?
            .into(),
        None => {
            if blocklist_rule::Entity::find().count(&transaction).await? >= MAX_RULES {
                return Err(Error::Full);
            }
            Default::default()
        }
    };
    active.kind = Set(kind.into());
    active.value = Set(value);
    active.note = Set(note.into());
    active.rule_key = Set(rule_key);
    let (saved, action) = if id.is_some() {
        (active.update(&transaction).await?, "blocklist.update")
    } else {
        (active.insert(&transaction).await?, "blocklist.create")
    };
    console_audit::record(&transaction, action, "success", actor_id, Some(saved.id)).await?;
    transaction.commit().await?;
    Ok(saved)
}

pub(crate) async fn remove(db: &DatabaseConnection, actor_id: i32, id: i32) -> Result<(), Error> {
    let transaction = console_roles::authority_transaction(db).await?;
    if blocklist_rule::Entity::delete_by_id(id)
        .exec(&transaction)
        .await?
        .rows_affected
        == 0
    {
        return Err(Error::NotFound);
    }
    console_audit::record(
        &transaction,
        "blocklist.delete",
        "success",
        actor_id,
        Some(id),
    )
    .await?;
    transaction.commit().await?;
    Ok(())
}

#[derive(Debug, Serialize)]
pub(crate) struct Snapshot<'a> {
    version: u8,
    pub revision: String,
    rules: Vec<SnapshotRule<'a>>,
}

#[derive(Debug, Serialize)]
struct SnapshotRule<'a> {
    kind: &'a str,
    value: &'a str,
}

impl<'a> Snapshot<'a> {
    pub(crate) fn new(rules: &'a [blocklist_rule::Model]) -> Result<Self, Error> {
        if rules.len() > MAX_RULES as usize {
            return Err(Error::Full);
        }
        let mut digest = Sha256::new();
        let rules = rules
            .iter()
            .map(|rule| {
                digest.update(rule.rule_key.as_bytes());
                SnapshotRule {
                    kind: &rule.kind,
                    value: &rule.value,
                }
            })
            .collect();
        Ok(Self {
            version: 1,
            revision: format!("{:x}", digest.finalize()),
            rules,
        })
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Agent {
    Hbbs,
    Hbbr,
}

#[derive(Debug)]
struct AppliedRevision {
    reported_at: Instant,
    revision: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct Pairing(Mutex<[Option<AppliedRevision>; 2]>);

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SyncState {
    Waiting,
    Updating,
    Synced,
}

impl Pairing {
    pub(crate) async fn received(&self, agent: Agent, applied: Option<String>) {
        self.0.lock().await[agent as usize] = Some(AppliedRevision {
            reported_at: Instant::now(),
            revision: applied,
        });
    }

    pub(crate) async fn status(&self, revision: &str) -> [SyncState; 2] {
        self.0.lock().await.each_ref().map(|report| match report {
            Some(report) if report.reported_at.elapsed() <= PAIRING_TIMEOUT => {
                if report.revision.as_deref() == Some(revision) {
                    SyncState::Synced
                } else {
                    SyncState::Updating
                }
            }
            _ => SyncState::Waiting,
        })
    }
}

#[derive(Debug, FromQueryResult, Serialize)]
pub(crate) struct DeviceChoice {
    id: String,
    hostname: String,
    alias: String,
    last_online_ip: String,
}

pub(crate) async fn devices(
    db: &DatabaseConnection,
    search: &str,
) -> Result<Vec<DeviceChoice>, DbErr> {
    use peer::Column::{Alias, Hostname, Id, LastOnlineIp};
    let mut query = peer::Entity::find()
        .select_only()
        .columns([Id, Hostname, Alias, LastOnlineIp])
        .filter(Id.ne(""));
    if !search.is_empty() {
        query = query.filter(
            Condition::any()
                .add(Id.contains(search))
                .add(Hostname.contains(search))
                .add(Alias.contains(search)),
        );
    }
    query
        .order_by_desc(peer::Column::LastOnlineTime)
        .limit(20)
        .into_model()
        .all(db)
        .await
}
