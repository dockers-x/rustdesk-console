import { Badge } from "@cloudflare/kumo/components/badge";
import { ActiveConnectionActions } from "../components/ActiveConnectionActions";
import { DeploymentTokenActions } from "../components/DeploymentTokenActions";
import { OAuthProviderActions } from "../components/OAuthProviderActions";
import { PeerQuickActions } from "../components/PeerQuickActions";
import { OAuthProviderBadge } from "../components/OAuthProviderBadge";
import { RecordFileActions } from "../components/RecordFileActions";
import { WebClientActions } from "../components/WebClientActions";
import { renderStrategyOptionsSummary } from "./strategyOptions";
import type { ResourceConfig } from "./types";

const statusCol = {
  key: "status",
  label: "status",
  render: (r: Record<string, unknown>, t: (k: string) => string) =>
    r.status === 1 ? t("enabled") : t("disabled"),
};
const boolCol = (key: string, label: string) => ({
  key,
  label,
  render: (r: Record<string, unknown>) => (r[key] ? "✓" : "—"),
});
const monoCol = (key: string, label: string, width = "max-w-44") => ({
  key,
  label,
  render: (r: Record<string, unknown>) => {
    const value = String(r[key] ?? "");
    if (!value) return "—";
    return (
      <span
        className={`block ${width} truncate font-mono text-xs tabular-nums`}
        title={value}
      >
        {value}
      </span>
    );
  },
});
const adminCol = {
  key: "is_admin",
  label: "isAdmin",
  render: (r: Record<string, unknown>, t: (k: string) => string) =>
    r.is_admin ? <Badge>{t("isAdmin")}</Badge> : "—",
};
const webClientCol = (share = false) => ({
  key: "__webclient",
  label: "webClient",
  render: (r: Record<string, unknown>) => (
    <WebClientActions peerId={String(r.id ?? "")} share={share} />
  ),
});
const myPeerActionsCol = {
  key: "__my_peer_actions",
  label: "quickActions",
  render: (r: Record<string, unknown>) => (
    <PeerQuickActions
      peerId={String(r.id ?? "")}
      rowId={Number(r.row_id ?? 0)}
    />
  ),
};
const activeConnectionActionsCol = {
  key: "__active_connection_actions",
  label: "actions",
  render: (r: Record<string, unknown>) => (
    <ActiveConnectionActions
      connId={Number(r.conn_id ?? 0)}
      peerId={String(r.peer_id ?? "")}
      uuid={String(r.uuid ?? "")}
    />
  ),
};
const colorCol = {
  key: "color",
  label: "color",
  render: (r: Record<string, unknown>) => {
    const n = Number(r.color);
    if (!Number.isFinite(n) || n <= 0) return "—";
    const hex = `#${((n >>> 0) & 0x00ffffff).toString(16).padStart(6, "0")}`;
    return (
      <span className="inline-flex items-center gap-2 tabular-nums">
        <span
          className="size-4 rounded border border-kumo-line"
          style={{ backgroundColor: hex }}
          aria-hidden="true"
        />
        <span className="font-mono text-xs">{hex}</span>
      </span>
    );
  },
};
const strategyTargetCol = {
  key: "target_type",
  label: "targetType",
  render: (r: Record<string, unknown>, t: (k: string) => string) => {
    switch (String(r.target_type ?? "")) {
      case "peer":
        return t("targetPeer");
      case "user":
        return t("targetUser");
      case "device_group":
        return t("targetDeviceGroup");
      default:
        return String(r.target_type ?? "") || "—";
    }
  },
};
const RULE_OPTIONS = [
  { label: "ruleRead", value: 1 },
  { label: "ruleReadWrite", value: 2 },
  { label: "ruleFull", value: 3 },
];
const RULE_TYPE_OPTIONS = [
  { label: "rulePersonal", value: 1 },
  { label: "ruleGroup", value: 2 },
];
const STRATEGY_TARGET_OPTIONS = [
  { label: "targetPeer", value: "peer" },
  { label: "targetUser", value: "user" },
  { label: "targetDeviceGroup", value: "device_group" },
];
const USER_RELATION = {
  api: "/api/admin/user",
  labelFields: ["username", "nickname", "email", "id"],
};
const GROUP_RELATION = {
  api: "/api/admin/group",
  labelFields: ["name", "id"],
  includeEmptyOption: false,
};
const DEVICE_GROUP_RELATION = {
  api: "/api/admin/device_group",
  labelFields: ["name", "id"],
};
const STRATEGY_RELATION = {
  api: "/api/admin/strategy",
  labelFields: ["name", "guid", "id"],
};
const COLLECTION_RELATION = {
  api: "/api/admin/address_book_collection",
  labelFields: ["name", "id"],
};
const oauthTypeIs =
  (...types: string[]) =>
  (form: Record<string, unknown>) =>
    types.includes(String(form.oauth_type ?? ""));
const strategyTargetIs =
  (...types: string[]) =>
  (form: Record<string, unknown>) =>
    types.includes(String(form.target_type ?? ""));

export function resourcePath(r: ResourceConfig) {
  return r.path ?? `/${r.name}`;
}

/// Every config-driven admin screen. Adding a screen = adding an entry here.
export const ADMIN_RESOURCES: ResourceConfig[] = [
  {
    name: "users",
    titleKey: "users",
    api: "/api/admin/user",
    idField: "id",
    filters: [{ name: "username", label: "search" }],
    columns: [
      { key: "id", label: "id" },
      { key: "username", label: "username" },
      { key: "email", label: "email" },
      { key: "nickname", label: "nickname" },
      adminCol,
      statusCol,
      { key: "created_at", label: "createdAt" },
    ],
    fields: [
      { name: "username", label: "username", type: "text", lockOnEdit: true },
      { name: "password", label: "password", type: "password", createOnly: true },
      { name: "email", label: "email", type: "text" },
      { name: "nickname", label: "nickname", type: "text" },
      {
        name: "avatar",
        label: "avatar",
        type: "avatar",
        placeholder: "avatarPlaceholder",
        hint: "avatarHint",
      },
      {
        name: "group_id",
        label: "groupId",
        type: "relation",
        relation: GROUP_RELATION,
        defaultValue: 1,
      },
      { name: "is_admin", label: "isAdmin", type: "switch" },
      {
        name: "status",
        label: "status",
        type: "switch",
        switchOn: 1,
        switchOff: 2,
        defaultValue: 1,
      },
    ],
  },
  {
    name: "groups",
    titleKey: "groups",
    api: "/api/admin/group",
    columns: [
      { key: "id", label: "id" },
      { key: "name", label: "name" },
      {
        key: "type",
        label: "type",
        render: (r, t) => (r.type === 2 ? t("groupShare") : t("groupDefault")),
      },
      { key: "created_at", label: "createdAt" },
    ],
    fields: [
      { name: "name", label: "name", type: "text" },
      {
        name: "type",
        label: "type",
        type: "select",
        defaultValue: 1,
        options: [
          { label: "groupDefault", value: 1 },
          { label: "groupShare", value: 2 },
        ],
      },
    ],
  },
  {
    name: "device_groups",
    titleKey: "deviceGroups",
    api: "/api/admin/device_group",
    columns: [
      { key: "id", label: "id" },
      { key: "name", label: "name" },
      { key: "created_at", label: "createdAt" },
    ],
    fields: [{ name: "name", label: "name", type: "text" }],
  },
  {
    name: "deployment_tokens",
    titleKey: "deploymentTokens",
    api: "/api/admin/deployment_token",
    canEdit: false,
    filters: [{ name: "name", label: "search" }],
    columns: [
      { key: "id", label: "id" },
      { key: "name", label: "name" },
      monoCol("scopes", "scopes", "max-w-64"),
      { key: "default_user_id", label: "defaultUserId" },
      { key: "default_device_group_id", label: "defaultDeviceGroupId" },
      { key: "default_strategy_id", label: "defaultStrategyId" },
      { key: "used_count", label: "usedCount" },
      { key: "max_uses", label: "maxUses" },
      { key: "expires_at", label: "expiredAt" },
      { key: "revoked_at", label: "revokedAt" },
      { key: "created_at", label: "createdAt" },
    ],
    fields: [
      { name: "name", label: "name", type: "text" },
      {
        name: "scopes",
        label: "scopes",
        type: "textarea",
        defaultValue: "deploy,assign,strategy_assign,address_book_assign",
        hint: "deploymentTokenScopesHint",
      },
      {
        name: "default_user_id",
        label: "defaultUserId",
        type: "relation",
        relation: USER_RELATION,
        defaultValue: 0,
        hint: "deploymentTokenDefaultUserHint",
      },
      {
        name: "default_device_group_id",
        label: "defaultDeviceGroupId",
        type: "relation",
        relation: DEVICE_GROUP_RELATION,
        defaultValue: 0,
      },
      {
        name: "default_strategy_id",
        label: "defaultStrategyId",
        type: "relation",
        relation: STRATEGY_RELATION,
        defaultValue: 0,
      },
      { name: "expires_at", label: "expiredAt", type: "number", defaultValue: 0 },
      { name: "max_uses", label: "maxUses", type: "number", defaultValue: 0 },
    ],
    rowActions: (r) => <DeploymentTokenActions id={Number(r.id ?? 0)} />,
  },
  {
    name: "strategies",
    titleKey: "strategies",
    api: "/api/admin/strategy",
    filters: [{ name: "name", label: "search" }],
    columns: [
      { key: "id", label: "id" },
      monoCol("guid", "guid", "max-w-52"),
      { key: "name", label: "name" },
      statusCol,
      { key: "modified_at", label: "modifiedAt" },
      {
        key: "config_options",
        label: "configOptions",
        render: (r, t) => renderStrategyOptionsSummary(r.config_options, t),
      },
      { key: "note", label: "note" },
      { key: "created_at", label: "createdAt" },
    ],
    fields: [
      { name: "name", label: "name", type: "text" },
      { name: "note", label: "note", type: "text" },
      {
        name: "status",
        label: "status",
        type: "switch",
        switchOn: 1,
        switchOff: 2,
        defaultValue: 1,
      },
      {
        name: "config_options",
        label: "configOptions",
        type: "strategy_options",
        defaultValue: {},
        hint: "strategyOptionsHint",
      },
      {
        name: "extra",
        label: "extra",
        type: "textarea",
        defaultValue: "{}",
        hint: "strategyExtraHint",
      },
    ],
  },
  {
    name: "strategy_assignments",
    titleKey: "strategyAssignments",
    api: "/api/admin/strategy_assignment",
    columns: [
      { key: "id", label: "id" },
      { key: "strategy_id", label: "strategyId" },
      strategyTargetCol,
      monoCol("target_id", "targetId", "max-w-44"),
      { key: "priority", label: "priority" },
      { key: "created_at", label: "createdAt" },
    ],
    fields: [
      {
        name: "strategy_id",
        label: "strategyId",
        type: "relation",
        relation: STRATEGY_RELATION,
        defaultValue: 0,
      },
      {
        name: "target_type",
        label: "targetType",
        type: "select",
        defaultValue: "peer",
        options: STRATEGY_TARGET_OPTIONS,
        resetFieldsOnChange: ["target_id"],
      },
      {
        name: "target_id",
        label: "targetId",
        type: "relation",
        relation: {
          api: (form) => {
            switch (String(form.target_type ?? "peer")) {
              case "user":
                return "/api/admin/user";
              case "device_group":
                return "/api/admin/device_group";
              default:
                return "/api/admin/peer";
            }
          },
          labelFields: ["hostname", "username", "name", "nickname", "id"],
          valueAsString: true,
          includeEmptyOption: false,
        },
        defaultValue: "",
        visibleWhen: strategyTargetIs("peer", "user", "device_group"),
      },
      { name: "priority", label: "priority", type: "number", defaultValue: 100 },
    ],
  },
  {
    name: "tags",
    titleKey: "tags",
    api: "/api/admin/tag",
    columns: [
      { key: "id", label: "id" },
      { key: "name", label: "name" },
      { key: "user_id", label: "userId" },
      colorCol,
      { key: "collection_id", label: "collectionId" },
    ],
    fields: [
      { name: "name", label: "name", type: "text" },
      {
        name: "user_id",
        label: "userId",
        type: "relation",
        relation: USER_RELATION,
      },
      {
        name: "color",
        label: "color",
        type: "color",
        defaultValue: 0,
        colorSeedField: "name",
      },
      {
        name: "collection_id",
        label: "collectionId",
        type: "relation",
        relation: COLLECTION_RELATION,
        defaultValue: 0,
      },
    ],
  },
  {
    name: "peers",
    titleKey: "devices",
    api: "/api/admin/peer",
    idField: "row_id",
    canCreate: false,
    filters: [
      { name: "id", label: "deviceId" },
      { name: "hostname", label: "hostname" },
    ],
    columns: [
      { key: "id", label: "deviceId" },
      { key: "hostname", label: "hostname" },
      { key: "os", label: "os" },
      { key: "username", label: "username" },
      { key: "last_online_ip", label: "ip" },
      { key: "group_id", label: "groupId" },
      { key: "alias", label: "alias" },
      webClientCol(false),
    ],
    fields: [
      { name: "alias", label: "alias", type: "text" },
      {
        name: "group_id",
        label: "groupId",
        type: "relation",
        relation: DEVICE_GROUP_RELATION,
        defaultValue: 0,
      },
      {
        name: "user_id",
        label: "userId",
        type: "relation",
        relation: USER_RELATION,
        defaultValue: 0,
      },
    ],
  },
  {
    name: "oauth",
    titleKey: "oauth",
    api: "/api/admin/oauth",
    columns: [
      { key: "id", label: "id" },
      { key: "op", label: "op" },
      {
        key: "oauth_type",
        label: "oauthType",
        render: (r) => <OAuthProviderBadge value={r.oauth_type} />,
      },
      boolCol("auto_register", "autoRegister"),
      boolCol("pkce_enable", "pkce"),
      { key: "client_id", label: "clientId" },
      { key: "issuer", label: "issuer" },
    ],
    rowActions: (row) => <OAuthProviderActions provider={row} />,
    fields: [
      {
        name: "oauth_type",
        label: "oauthType",
        type: "oauth_provider",
        defaultValue: "github",
        options: [
          { label: "github", value: "github" },
          { label: "google", value: "google" },
          { label: "oidc", value: "oidc" },
          { label: "linuxdo", value: "linuxdo" },
        ],
        hint: "oauthTypeHint",
      },
      {
        name: "op",
        label: "op",
        type: "text",
        placeholder: "oauthOpPlaceholder",
        hint: "oauthOpHint",
        visibleWhen: oauthTypeIs("oidc"),
      },
      {
        name: "client_id",
        label: "clientId",
        type: "text",
        hint: "oauthClientIdHint",
      },
      {
        name: "client_secret",
        label: "clientSecret",
        type: "password",
        hint: "oauthClientSecretHint",
      },
      {
        name: "issuer",
        label: "issuer",
        type: "text",
        placeholder: "oauthIssuerPlaceholder",
        hint: "oauthIssuerHint",
        visibleWhen: oauthTypeIs("google", "oidc"),
      },
      {
        name: "scopes",
        label: "scopes",
        type: "text",
        defaultValue: "openid,profile,email",
        hint: "oauthScopesHint",
        visibleWhen: oauthTypeIs("google", "oidc"),
      },
      {
        name: "auto_register",
        label: "autoRegister",
        type: "switch",
        hint: "oauthAutoRegisterHint",
      },
      {
        name: "pkce_enable",
        label: "pkce",
        type: "switch",
        hint: "oauthPkceHint",
        visibleWhen: oauthTypeIs("google", "oidc"),
      },
      {
        name: "pkce_method",
        label: "pkceMethod",
        type: "text",
        defaultValue: "S256",
        hint: "oauthPkceMethodHint",
        visibleWhen: oauthTypeIs("google", "oidc"),
      },
    ],
  },
  {
    name: "address_book",
    titleKey: "addressBook",
    api: "/api/admin/address_book",
    idField: "row_id",
    filters: [
      { name: "id", label: "deviceId" },
      { name: "username", label: "username" },
      { name: "hostname", label: "hostname" },
    ],
    columns: [
      { key: "id", label: "deviceId" },
      { key: "username", label: "username" },
      { key: "hostname", label: "hostname" },
      { key: "platform", label: "platform" },
      { key: "user_id", label: "userId" },
      { key: "collection_id", label: "collectionId" },
      webClientCol(false),
    ],
    fields: [
      { name: "id", label: "deviceId", type: "text", lockOnEdit: true },
      { name: "username", label: "username", type: "text" },
      { name: "hostname", label: "hostname", type: "text" },
      { name: "platform", label: "platform", type: "text" },
      { name: "alias", label: "alias", type: "text" },
      {
        name: "user_id",
        label: "userId",
        type: "relation",
        relation: USER_RELATION,
        defaultValue: 0,
      },
      {
        name: "collection_id",
        label: "collectionId",
        type: "relation",
        relation: {
          ...COLLECTION_RELATION,
          params: (form) => ({ user_id: form.user_id || 0 }),
        },
        defaultValue: 0,
      },
    ],
  },
  {
    name: "address_book_collection",
    titleKey: "collections",
    api: "/api/admin/address_book_collection",
    columns: [
      { key: "id", label: "id" },
      { key: "name", label: "name" },
      { key: "user_id", label: "userId" },
      { key: "created_at", label: "createdAt" },
    ],
    fields: [
      { name: "name", label: "name", type: "text" },
      {
        name: "user_id",
        label: "userId",
        type: "relation",
        relation: USER_RELATION,
      },
    ],
  },
  {
    name: "address_book_collection_rule",
    titleKey: "shareRules",
    api: "/api/admin/address_book_collection_rule",
    columns: [
      { key: "id", label: "id" },
      { key: "user_id", label: "userId" },
      { key: "collection_id", label: "collectionId" },
      {
        key: "rule",
        label: "rule",
        render: (r, t) =>
          t(["", "ruleRead", "ruleReadWrite", "ruleFull"][r.rule as number] || "—"),
      },
      {
        key: "type",
        label: "type",
        render: (r, t) => (r.type === 2 ? t("ruleGroup") : t("rulePersonal")),
      },
      { key: "to_id", label: "toId" },
    ],
    fields: [
      {
        name: "user_id",
        label: "userId",
        type: "relation",
        relation: USER_RELATION,
      },
      {
        name: "collection_id",
        label: "collectionId",
        type: "relation",
        relation: {
          ...COLLECTION_RELATION,
          params: (form) => ({ user_id: form.user_id || 0 }),
        },
      },
      { name: "rule", label: "rule", type: "select", defaultValue: 1, options: RULE_OPTIONS },
      { name: "type", label: "type", type: "select", defaultValue: 1, options: RULE_TYPE_OPTIONS },
      {
        name: "to_id",
        label: "toId",
        type: "relation",
        relation: {
          api: (form) =>
            Number(form.type ?? 1) === 2 ? "/api/admin/group" : "/api/admin/user",
          labelFields: ["name", "username", "nickname", "id"],
          includeEmptyOption: false,
        },
      },
    ],
  },
  {
    name: "share_record",
    titleKey: "shareRecords",
    api: "/api/admin/share_record",
    canCreate: false,
    canEdit: false,
    columns: [
      { key: "id", label: "id" },
      { key: "user_id", label: "userId" },
      { key: "peer_id", label: "deviceId" },
      { key: "share_token", label: "shareToken" },
      { key: "password_type", label: "passwordType" },
      { key: "expire", label: "expire" },
      { key: "created_at", label: "createdAt" },
    ],
    fields: [],
  },
  {
    name: "user_token",
    titleKey: "userTokens",
    api: "/api/admin/user_token",
    canCreate: false,
    canEdit: false,
    columns: [
      { key: "id", label: "id" },
      { key: "user_id", label: "userId" },
      { key: "device_id", label: "deviceId" },
      { key: "device_uuid", label: "uuid" },
      { key: "expired_at", label: "expiredAt" },
      { key: "created_at", label: "createdAt" },
    ],
    fields: [],
  },
  {
    name: "login_log",
    titleKey: "loginLogs",
    api: "/api/admin/login_log",
    canCreate: false,
    canEdit: false,
    columns: [
      { key: "id", label: "id" },
      { key: "user_id", label: "userId" },
      { key: "client", label: "client" },
      { key: "device_id", label: "deviceId" },
      { key: "ip", label: "ip" },
      { key: "type", label: "type" },
      { key: "platform", label: "platform" },
      { key: "created_at", label: "createdAt" },
    ],
    fields: [],
  },
  {
    name: "active_connection",
    titleKey: "activeConnections",
    api: "/api/admin/active_connection",
    canCreate: false,
    canEdit: false,
    canDelete: false,
    filters: [
      { name: "peer_id", label: "deviceId" },
      { name: "uuid", label: "uuid" },
    ],
    columns: [
      { key: "id", label: "id" },
      monoCol("conn_id", "connId", "max-w-24"),
      activeConnectionActionsCol,
      monoCol("peer_id", "deviceId", "max-w-28"),
      monoCol("uuid", "uuid", "max-w-48"),
      { key: "updated_at", label: "updatedAt" },
    ],
    fields: [],
  },
  {
    name: "audit_conn",
    titleKey: "auditConn",
    api: "/api/admin/audit_conn",
    canCreate: false,
    canEdit: false,
    filters: [
      { name: "peer_id", label: "deviceId" },
      { name: "from_peer", label: "fromPeer" },
    ],
    columns: [
      { key: "id", label: "id" },
      { key: "action", label: "action" },
      { key: "peer_id", label: "deviceId" },
      { key: "from_peer", label: "fromPeer" },
      { key: "from_name", label: "fromName" },
      { key: "ip", label: "ip" },
      { key: "created_at", label: "createdAt" },
    ],
    fields: [],
  },
  {
    name: "audit_file",
    titleKey: "auditFile",
    api: "/api/admin/audit_file",
    canCreate: false,
    canEdit: false,
    filters: [
      { name: "peer_id", label: "deviceId" },
      { name: "from_peer", label: "fromPeer" },
    ],
    columns: [
      { key: "id", label: "id" },
      { key: "peer_id", label: "deviceId" },
      { key: "from_peer", label: "fromPeer" },
      { key: "path", label: "path" },
      boolCol("is_file", "isFile"),
      { key: "num", label: "num" },
      { key: "created_at", label: "createdAt" },
    ],
    fields: [],
  },
  {
    name: "record_file",
    titleKey: "recordFiles",
    api: "/api/admin/record_file",
    canCreate: false,
    canEdit: false,
    filters: [
      { name: "filename", label: "filename" },
      { name: "peer_id", label: "deviceId" },
    ],
    columns: [
      { key: "id", label: "id" },
      monoCol("filename", "filename", "max-w-80"),
      monoCol("peer_id", "deviceId", "max-w-28"),
      { key: "direction", label: "direction" },
      { key: "storage_backend", label: "storageBackend" },
      { key: "size", label: "sizeBytes" },
      {
        key: "status",
        label: "status",
        render: (r, t) =>
          Number(r.status ?? 0) === 1
            ? t("recordStatusComplete")
            : t("recordStatusUploading"),
      },
      { key: "created_at", label: "createdAt" },
    ],
    fields: [],
    rowActions: (r) => (
      <RecordFileActions
        id={Number(r.id ?? 0)}
        filename={String(r.filename ?? "")}
      />
    ),
  },
];

export const MY_RESOURCES: ResourceConfig[] = [
  {
    name: "my_peer",
    path: "/my/peer",
    titleKey: "myPeers",
    api: "/api/admin/my/peer",
    idField: "row_id",
    canCreate: false,
    canEdit: false,
    canDelete: false,
    filters: [
      { name: "id", label: "deviceId" },
      { name: "hostname", label: "hostname" },
      { name: "username", label: "username" },
    ],
    columns: [
      { key: "id", label: "deviceId" },
      { key: "hostname", label: "hostname" },
      { key: "os", label: "os" },
      { key: "username", label: "username" },
      { key: "last_online_ip", label: "ip" },
      { key: "alias", label: "alias" },
      { key: "version", label: "version" },
      { key: "updated_at", label: "updatedAt" },
      myPeerActionsCol,
    ],
    fields: [],
  },
  {
    name: "my_address_book_collection",
    path: "/my/address_book_collection",
    titleKey: "myCollections",
    api: "/api/admin/my/address_book_collection",
    columns: [
      { key: "id", label: "id" },
      { key: "name", label: "name" },
      { key: "created_at", label: "createdAt" },
    ],
    fields: [{ name: "name", label: "name", type: "text" }],
  },
  {
    name: "my_address_book",
    path: "/my/address_book",
    titleKey: "myAddressBook",
    api: "/api/admin/my/address_book",
    idField: "row_id",
    filters: [
      { name: "id", label: "deviceId" },
      { name: "username", label: "username" },
      { name: "hostname", label: "hostname" },
      { name: "collection_id", label: "collectionId" },
    ],
    columns: [
      { key: "id", label: "deviceId" },
      { key: "username", label: "username" },
      { key: "hostname", label: "hostname" },
      { key: "platform", label: "platform" },
      { key: "alias", label: "alias" },
      { key: "collection_id", label: "collectionId" },
      webClientCol(true),
    ],
    fields: [
      { name: "id", label: "deviceId", type: "text", lockOnEdit: true },
      { name: "username", label: "username", type: "text" },
      { name: "hostname", label: "hostname", type: "text" },
      { name: "platform", label: "platform", type: "text" },
      { name: "alias", label: "alias", type: "text" },
      {
        name: "collection_id",
        label: "collectionId",
        type: "relation",
        relation: {
          api: "/api/admin/my/address_book_collection",
          labelFields: ["name", "id"],
        },
        defaultValue: 0,
      },
    ],
  },
  {
    name: "my_tag",
    path: "/my/tag",
    titleKey: "myTags",
    api: "/api/admin/my/tag",
    filters: [{ name: "collection_id", label: "collectionId" }],
    columns: [
      { key: "id", label: "id" },
      { key: "name", label: "name" },
      colorCol,
      { key: "collection_id", label: "collectionId" },
    ],
    fields: [
      { name: "name", label: "name", type: "text" },
      {
        name: "color",
        label: "color",
        type: "color",
        defaultValue: 0,
        colorSeedField: "name",
      },
      {
        name: "collection_id",
        label: "collectionId",
        type: "relation",
        relation: {
          api: "/api/admin/my/address_book_collection",
          labelFields: ["name", "id"],
        },
        defaultValue: 0,
      },
    ],
  },
  {
    name: "my_address_book_collection_rule",
    path: "/my/address_book_collection_rule",
    titleKey: "myShareRules",
    api: "/api/admin/my/address_book_collection_rule",
    filters: [{ name: "collection_id", label: "collectionId" }],
    columns: [
      { key: "id", label: "id" },
      { key: "collection_id", label: "collectionId" },
      {
        key: "rule",
        label: "rule",
        render: (r, t) =>
          t(["", "ruleRead", "ruleReadWrite", "ruleFull"][r.rule as number] || "—"),
      },
      {
        key: "type",
        label: "type",
        render: (r, t) => (r.type === 2 ? t("ruleGroup") : t("rulePersonal")),
      },
      { key: "to_id", label: "toId" },
    ],
    fields: [
      {
        name: "collection_id",
        label: "collectionId",
        type: "relation",
        relation: {
          api: "/api/admin/my/address_book_collection",
          labelFields: ["name", "id"],
        },
        defaultValue: 0,
      },
      { name: "rule", label: "rule", type: "select", defaultValue: 1, options: RULE_OPTIONS },
      { name: "type", label: "type", type: "select", defaultValue: 1, options: RULE_TYPE_OPTIONS },
      {
        name: "to_id",
        label: "toId",
        type: "relation",
        relation: {
          api: (form) =>
            Number(form.type ?? 1) === 2 ? "/api/admin/group" : "/api/admin/user",
          labelFields: ["name", "username", "nickname", "id"],
          includeEmptyOption: false,
        },
      },
    ],
  },
  {
    name: "my_share_record",
    path: "/my/shareRecord",
    titleKey: "myShareRecords",
    api: "/api/admin/my/share_record",
    canCreate: false,
    canEdit: false,
    columns: [
      { key: "id", label: "id" },
      { key: "peer_id", label: "deviceId" },
      { key: "share_token", label: "shareToken" },
      { key: "password_type", label: "passwordType" },
      { key: "expire", label: "expire" },
      { key: "created_at", label: "createdAt" },
    ],
    fields: [],
  },
  {
    name: "my_login_log",
    path: "/my/loginLog",
    titleKey: "myLoginLogs",
    api: "/api/admin/my/login_log",
    canCreate: false,
    canEdit: false,
    columns: [
      { key: "id", label: "id" },
      { key: "client", label: "client" },
      { key: "device_id", label: "deviceId" },
      { key: "ip", label: "ip" },
      { key: "type", label: "type" },
      { key: "platform", label: "platform" },
      { key: "created_at", label: "createdAt" },
    ],
    fields: [],
  },
];

export const RESOURCES = ADMIN_RESOURCES;
export const ALL_RESOURCES = [...ADMIN_RESOURCES, ...MY_RESOURCES];
