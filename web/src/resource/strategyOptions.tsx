import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Input } from "@cloudflare/kumo/components/input";
import { Check, Minus, X } from "@phosphor-icons/react";

type StrategyOptionMap = Record<string, string>;

type StrategyOptionType = "tri_state" | "select" | "number" | "whitelist";

interface StrategyOptionChoice {
  value: string;
  label: string;
}

interface StrategyOptionGroup {
  key: string;
  label: string;
}

interface StrategyOptionDefinition {
  key: string;
  label: string;
  hint?: string;
  group: string;
  type: StrategyOptionType;
  choices?: readonly StrategyOptionChoice[];
  defaultValue?: string;
  placeholder?: string;
}

const STRATEGY_OPTION_GROUPS: readonly StrategyOptionGroup[] = [
  { key: "access", label: "strategyOptionGroupAccess" },
  { key: "permissions", label: "strategyOptionGroupPermissions" },
  { key: "security", label: "strategyOptionGroupSecurity" },
  { key: "session", label: "strategyOptionGroupSession" },
];

const ACCESS_MODE_CHOICES = [
  { value: "custom", label: "strategyAccessCustom" },
  { value: "full", label: "strategyAccessFull" },
  { value: "view", label: "strategyAccessView" },
] as const;

const APPROVE_MODE_CHOICES = [
  { value: "password", label: "approveModePassword" },
  { value: "click", label: "approveModeClick" },
  { value: "password-click", label: "approveModePasswordClick" },
] as const;

const VERIFICATION_METHOD_CHOICES = [
  { value: "use-temporary-password", label: "verificationTemporaryOnly" },
  { value: "use-permanent-password", label: "verificationPermanentOnly" },
  { value: "use-both-passwords", label: "verificationBothPasswords" },
] as const;

const STRATEGY_OPTION_DEFINITIONS: readonly StrategyOptionDefinition[] = [
  {
    key: "access-mode",
    label: "strategyAccessMode",
    hint: "strategyAccessModeHint",
    group: "access",
    type: "select",
    choices: ACCESS_MODE_CHOICES,
  },
  {
    key: "enable-keyboard",
    label: "strategyKeyboardMouse",
    group: "permissions",
    type: "tri_state",
  },
  {
    key: "enable-clipboard",
    label: "strategyClipboard",
    group: "permissions",
    type: "tri_state",
  },
  {
    key: "enable-file-transfer",
    label: "strategyFileTransfer",
    group: "permissions",
    type: "tri_state",
  },
  {
    key: "enable-camera",
    label: "strategyCamera",
    group: "permissions",
    type: "tri_state",
  },
  {
    key: "enable-terminal",
    label: "strategyTerminal",
    group: "permissions",
    type: "tri_state",
  },
  {
    key: "enable-audio",
    label: "strategyAudio",
    group: "permissions",
    type: "tri_state",
  },
  {
    key: "enable-tunnel",
    label: "strategyTcpTunneling",
    group: "permissions",
    type: "tri_state",
  },
  {
    key: "enable-remote-restart",
    label: "strategyRemoteRestart",
    group: "permissions",
    type: "tri_state",
  },
  {
    key: "enable-record-session",
    label: "strategyRecordSession",
    group: "permissions",
    type: "tri_state",
  },
  {
    key: "enable-block-input",
    label: "strategyBlockInput",
    group: "permissions",
    type: "tri_state",
  },
  {
    key: "enable-privacy-mode",
    label: "strategyPrivacyMode",
    group: "permissions",
    type: "tri_state",
  },
  {
    key: "allow-remote-config-modification",
    label: "strategyRemoteConfigModification",
    group: "permissions",
    type: "tri_state",
  },
  {
    key: "approve-mode",
    label: "strategyApproveMode",
    group: "security",
    type: "select",
    choices: APPROVE_MODE_CHOICES,
  },
  {
    key: "verification-method",
    label: "strategyVerificationMethod",
    group: "security",
    type: "select",
    choices: VERIFICATION_METHOD_CHOICES,
  },
  {
    key: "whitelist",
    label: "strategyWhitelist",
    hint: "strategyWhitelistHint",
    group: "security",
    type: "whitelist",
    placeholder: "strategyWhitelistPlaceholder",
  },
  {
    key: "allow-auto-disconnect",
    label: "strategyAutoDisconnect",
    group: "session",
    type: "tri_state",
  },
  {
    key: "auto-disconnect-timeout",
    label: "strategyAutoDisconnectTimeout",
    hint: "strategyAutoDisconnectTimeoutHint",
    group: "session",
    type: "number",
    defaultValue: "10",
  },
];

const STRATEGY_OPTION_BY_KEY = new Map(
  STRATEGY_OPTION_DEFINITIONS.map((definition) => [
    definition.key,
    definition,
  ]),
);

export function parseStrategyOptionsValue(value: unknown): StrategyOptionMap {
  if (value === null || value === undefined) return {};
  if (typeof value === "object" && !Array.isArray(value)) {
    return Object.fromEntries(
      Object.entries(value)
        .map(([key, raw]) => [key.trim(), valueToString(raw)] as const)
        .filter(([key, raw]) => key !== "" && raw !== null)
        .map(([key, raw]) => [key, raw ?? ""]),
    );
  }
  const raw = String(value).trim();
  if (!raw) return {};
  try {
    const parsed = JSON.parse(raw) as unknown;
    return parseStrategyOptionsValue(parsed);
  } catch {
    return Object.fromEntries(
      raw
        .split(",")
        .map((pair) => pair.split("=", 2))
        .filter((pair): pair is [string, string] => pair.length === 2)
        .map(([key, optionValue]) => [key.trim(), optionValue.trim()])
        .filter(([key]) => key !== ""),
    );
  }
}

export function renderStrategyOptionsSummary(
  value: unknown,
  t: (key: string) => string,
): ReactNode {
  const entries = Object.entries(parseStrategyOptionsValue(value)).filter(
    ([, optionValue]) => optionValue !== "",
  );
  if (entries.length === 0) return "—";
  const visible = entries.slice(0, 3);
  const hiddenCount = entries.length - visible.length;
  return (
    <span className="flex max-w-72 flex-wrap gap-1">
      {visible.map(([key, optionValue]) => {
        const definition = STRATEGY_OPTION_BY_KEY.get(key);
        return (
          <span
            key={key}
            className="inline-flex max-w-full items-center gap-1 rounded border border-kumo-line bg-kumo-elevated px-1.5 py-0.5 text-xs"
            title={`${key}=${optionValue}`}
          >
            <span className="truncate">
              {definition ? t(definition.label) : key}
            </span>
            <span className="text-kumo-subtle">
              {optionValueLabel(definition, optionValue, t)}
            </span>
          </span>
        );
      })}
      {hiddenCount > 0 && (
        <span className="inline-flex rounded border border-kumo-line px-1.5 py-0.5 text-xs text-kumo-subtle">
          +{hiddenCount}
        </span>
      )}
    </span>
  );
}

export function StrategyOptionsInput({
  label,
  hint,
  value,
  onChange,
}: {
  label: string;
  hint?: string;
  value: unknown;
  onChange: (value: StrategyOptionMap) => void;
}) {
  const { t } = useTranslation();
  const options = parseStrategyOptionsValue(value);
  const configuredCount = Object.values(options).filter((v) => v !== "").length;

  const updateOption = (key: string, nextValue: string | undefined) => {
    const next = { ...options };
    if (nextValue === undefined || nextValue.trim() === "") {
      delete next[key];
    } else {
      next[key] = nextValue.trim();
    }
    onChange(next);
  };

  return (
    <div className="rounded-lg border border-kumo-line bg-kumo-base">
      <div className="border-b border-kumo-line px-3 py-3">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <span className="text-sm font-medium">{label}</span>
          <span className="rounded border border-kumo-line bg-kumo-elevated px-2 py-0.5 text-xs text-kumo-subtle">
            {t("strategyConfiguredCount", { count: configuredCount })}
          </span>
        </div>
        {hint && (
          <span className="mt-1 block text-xs text-kumo-subtle">{hint}</span>
        )}
      </div>
      <div className="divide-y divide-kumo-line">
        {STRATEGY_OPTION_GROUPS.map((group) => {
          const definitions = STRATEGY_OPTION_DEFINITIONS.filter(
            (definition) => definition.group === group.key,
          );
          return (
            <section key={group.key} className="px-3 py-3">
              <h3 className="mb-1 text-xs font-semibold uppercase text-kumo-subtle">
                {t(group.label)}
              </h3>
              <div className="divide-y divide-kumo-line">
                {definitions.map((definition) => (
                  <StrategyOptionRow
                    key={definition.key}
                    definition={definition}
                    value={options[definition.key]}
                    onChange={(nextValue) =>
                      updateOption(definition.key, nextValue)
                    }
                  />
                ))}
              </div>
            </section>
          );
        })}
      </div>
    </div>
  );
}

function StrategyOptionRow({
  definition,
  value,
  onChange,
}: {
  definition: StrategyOptionDefinition;
  value: string | undefined;
  onChange: (value: string | undefined) => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="grid gap-3 py-3 sm:grid-cols-[minmax(0,1fr)_minmax(15rem,20rem)] sm:items-start">
      <div className="min-w-0">
        <div className="text-sm font-medium">{t(definition.label)}</div>
        {definition.hint && (
          <p className="mt-1 text-xs leading-5 text-kumo-subtle">
            {t(definition.hint)}
          </p>
        )}
      </div>
      <StrategyOptionControl
        definition={definition}
        value={value}
        onChange={onChange}
      />
    </div>
  );
}

function StrategyOptionControl({
  definition,
  value,
  onChange,
}: {
  definition: StrategyOptionDefinition;
  value: string | undefined;
  onChange: (value: string | undefined) => void;
}) {
  const { t } = useTranslation();
  if (definition.type === "tri_state") {
    const current = value === "Y" || value === "N" ? value : "";
    const choices = [
      {
        value: "",
        label: t("strategyNotConfigured"),
        Icon: Minus,
        selectedClass:
          "bg-kumo-tint font-semibold text-kumo-default ring-1 ring-inset ring-kumo-line",
      },
      {
        value: "Y",
        label: t("enabled"),
        Icon: Check,
        selectedClass: "bg-kumo-brand font-semibold text-white shadow-sm",
      },
      {
        value: "N",
        label: t("disabled"),
        Icon: X,
        selectedClass:
          "bg-kumo-danger/10 font-semibold text-kumo-danger ring-1 ring-inset ring-kumo-danger/30",
      },
    ];
    return (
      <div
        className="grid min-h-10 grid-cols-3 overflow-hidden rounded-lg border border-kumo-line bg-kumo-elevated"
        role="group"
        aria-label={t(definition.label)}
      >
        {choices.map((choice) => {
          const selected = current === choice.value;
          const Icon = choice.Icon;
          return (
            <button
              key={choice.value || "unset"}
              type="button"
              aria-pressed={selected}
              title={choice.label}
              className={[
                "relative inline-flex min-w-0 items-center justify-center gap-1.5 border-r border-kumo-line px-2 text-sm transition-colors last:border-r-0 focus:outline-none focus-visible:z-10 focus-visible:ring-2 focus-visible:ring-kumo-brand",
                selected
                  ? `z-10 ${choice.selectedClass}`
                  : "bg-kumo-elevated text-kumo-subtle hover:bg-kumo-tint/70 hover:text-kumo-default",
              ].join(" ")}
              onClick={() =>
                onChange(choice.value === "" ? undefined : choice.value)
              }
            >
              <Icon size={14} weight={selected ? "bold" : "regular"} />
              <span className="truncate">{choice.label}</span>
            </button>
          );
        })}
      </div>
    );
  }

  if (definition.type === "select") {
    return (
      <select
        className="h-9 w-full rounded-lg border border-kumo-line bg-kumo-elevated px-3 text-sm focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand"
        value={value ?? ""}
        aria-label={t(definition.label)}
        onChange={(e) =>
          onChange(e.target.value === "" ? undefined : e.target.value)
        }
      >
        <option value="">{t("strategyNotConfigured")}</option>
        {(definition.choices ?? []).map((choice) => (
          <option key={choice.value} value={choice.value}>
            {t(choice.label)}
          </option>
        ))}
      </select>
    );
  }

  if (definition.type === "number") {
    const configured = value !== undefined && value !== "";
    return (
      <div className="flex min-w-0 flex-col gap-2 sm:flex-row">
        <select
          className="h-9 rounded-lg border border-kumo-line bg-kumo-elevated px-3 text-sm focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand sm:w-32"
          value={configured ? "set" : ""}
          aria-label={t(definition.label)}
          onChange={(e) =>
            onChange(
              e.target.value === ""
                ? undefined
                : (definition.defaultValue ?? "0"),
            )
          }
        >
          <option value="">{t("strategyNotConfigured")}</option>
          <option value="set">{t("strategySetValue")}</option>
        </select>
        {configured && (
          <Input
            aria-label={t(definition.label)}
            className="min-w-0 flex-1"
            type="number"
            min={0}
            max={65535}
            value={value}
            onChange={(e) => {
              const raw = e.target.value.trim();
              if (!raw) {
                onChange(undefined);
                return;
              }
              const numeric = Number(raw);
              if (!Number.isFinite(numeric)) return;
              const clamped = Math.max(0, Math.min(65535, Math.trunc(numeric)));
              onChange(String(clamped));
            }}
          />
        )}
      </div>
    );
  }

  const configured = value !== undefined && value !== "";
  return (
    <div className="flex min-w-0 flex-col gap-2">
      <select
        className="h-9 rounded-lg border border-kumo-line bg-kumo-elevated px-3 text-sm focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand"
        value={configured ? "set" : ""}
        aria-label={t(definition.label)}
        onChange={(e) => onChange(e.target.value === "" ? undefined : "0.0.0.0")}
      >
        <option value="">{t("strategyNotConfigured")}</option>
        <option value="set">{t("strategySetValue")}</option>
      </select>
      {configured && (
        <textarea
          className="min-h-20 w-full rounded-lg border border-kumo-line bg-kumo-elevated px-3 py-2 text-sm focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand"
          value={whitelistToText(value)}
          placeholder={
            definition.placeholder ? t(definition.placeholder) : undefined
          }
          aria-label={t(definition.label)}
          onChange={(e) => onChange(normalizeWhitelist(e.target.value))}
        />
      )}
    </div>
  );
}

function valueToString(value: unknown): string | null {
  if (value === null || value === undefined) return null;
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  return null;
}

function optionValueLabel(
  definition: StrategyOptionDefinition | undefined,
  value: string,
  t: (key: string) => string,
) {
  if (!definition) return value;
  if (definition.type === "tri_state") {
    if (value === "Y") return t("enabled");
    if (value === "N") return t("disabled");
  }
  if (definition.type === "select") {
    return t(definition.choices?.find((choice) => choice.value === value)?.label ?? "strategyConfigured");
  }
  if (definition.type === "number") return `${value} ${t("strategyMinutes")}`;
  return t("strategyConfigured");
}

function normalizeWhitelist(value: string) {
  return value
    .split(/[,\s;]+/)
    .map((item) => item.trim())
    .filter(Boolean)
    .join(",");
}

function whitelistToText(value: string | undefined) {
  return (value ?? "")
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean)
    .join("\n");
}
