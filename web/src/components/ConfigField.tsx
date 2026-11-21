import { Input } from "@cloudflare/kumo/components/input";

export function ConfigField({
  label,
  storageKey,
  value,
  onChange,
  placeholder,
  helper,
  disabled,
  compact = false,
}: {
  label: string;
  storageKey?: string;
  value: string;
  onChange: (value: string) => void;
  placeholder: string;
  helper?: string;
  compact?: boolean;
  disabled?: boolean;
}) {
  return (
    <label className="block min-w-0">
      <span className="mb-1 block text-sm font-medium">{label}</span>
      <Input
        aria-label={label}
        size={compact ? "sm" : "base"}
        value={value}
        disabled={disabled}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        spellCheck={false}
        className="w-full min-w-0"
      />
      {!compact && (storageKey || helper) && (
        <span className="mt-1 block text-xs leading-5 text-kumo-subtle">
          {storageKey}
          {storageKey && helper ? ". " : ""}{helper}
        </span>
      )}
    </label>
  );
}
