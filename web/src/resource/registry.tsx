import { Badge } from "@cloudflare/kumo/components/badge";
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
const adminCol = {
  key: "is_admin",
  label: "isAdmin",
  render: (r: Record<string, unknown>, t: (k: string) => string) =>
    r.is_admin ? <Badge>{t("isAdmin")}</Badge> : "—",
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

/// Every config-driven admin screen. Adding a screen = adding an entry here.
export const RESOURCES: ResourceConfig[] = [
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
      { name: "group_id", label: "groupId", type: "number", defaultValue: 1 },
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
    name: "tags",
    titleKey: "tags",
    api: "/api/admin/tag",
    columns: [
      { key: "id", label: "id" },
      { key: "name", label: "name" },
      { key: "user_id", label: "userId" },
      { key: "color", label: "color" },
      { key: "collection_id", label: "collectionId" },
    ],
    fields: [
      { name: "name", label: "name", type: "text" },
      { name: "user_id", label: "userId", type: "number" },
      { name: "color", label: "color", type: "number", defaultValue: 0 },
      { name: "collection_id", label: "collectionId", type: "number", defaultValue: 0 },
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
    ],
    fields: [
      { name: "alias", label: "alias", type: "text" },
      { name: "group_id", label: "groupId", type: "number", defaultValue: 0 },
      { name: "user_id", label: "userId", type: "number", defaultValue: 0 },
    ],
  },
  {
    name: "oauth",
    titleKey: "oauth",
    api: "/api/admin/oauth",
    columns: [
      { key: "id", label: "id" },
      { key: "op", label: "op" },
      { key: "oauth_type", label: "oauthType" },
      { key: "client_id", label: "clientId" },
      { key: "issuer", label: "issuer" },
    ],
    fields: [
      {
        name: "oauth_type",
        label: "oauthType",
        type: "select",
        defaultValue: "github",
        options: [
          { label: "github", value: "github" },
          { label: "google", value: "google" },
          { label: "oidc", value: "oidc" },
          { label: "linuxdo", value: "linuxdo" },
        ],
      },
      { name: "op", label: "op", type: "text" },
      { name: "client_id", label: "clientId", type: "text" },
      { name: "client_secret", label: "clientSecret", type: "text" },
      { name: "issuer", label: "issuer", type: "text" },
      { name: "scopes", label: "scopes", type: "text" },
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
    ],
    fields: [
      { name: "id", label: "deviceId", type: "text", lockOnEdit: true },
      { name: "username", label: "username", type: "text" },
      { name: "hostname", label: "hostname", type: "text" },
      { name: "platform", label: "platform", type: "text" },
      { name: "alias", label: "alias", type: "text" },
      { name: "user_id", label: "userId", type: "number", defaultValue: 0 },
      { name: "collection_id", label: "collectionId", type: "number", defaultValue: 0 },
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
      { name: "user_id", label: "userId", type: "number" },
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
      { name: "user_id", label: "userId", type: "number" },
      { name: "collection_id", label: "collectionId", type: "number" },
      { name: "rule", label: "rule", type: "select", defaultValue: 1, options: RULE_OPTIONS },
      { name: "type", label: "type", type: "select", defaultValue: 1, options: RULE_TYPE_OPTIONS },
      { name: "to_id", label: "toId", type: "number" },
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
];
