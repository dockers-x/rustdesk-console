import { useRef } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@cloudflare/kumo/components/button";
import { TagInput } from "@cloudflare/kumo/components/tag-input";

export function TagPicker({ label, value, options, onChange, description, disabled, allowCustom, maxValues }: {
  label: string;
  value: string[];
  options: readonly string[];
  onChange: (values: string[]) => void;
  description?: string;
  disabled?: boolean;
  allowCustom?: boolean;
  maxValues?: number;
}) {
  const { t } = useTranslation();
  const input = useRef<HTMLInputElement>(null);
  const choices = options.filter((option) => !value.includes(option));
  return (
    <div className="min-w-0 space-y-2">
      <TagInput
        ref={input}
        label={label}
        description={description}
        value={value}
        onValueChange={onChange}
        disabled={disabled}
        maxValues={maxValues}
        validateValue={(entry) => entry.length <= 80 && (allowCustom || options.includes(entry))}
        placeholder={t(allowCustom ? "tagInputPlaceholder" : "tagPickerPlaceholder")}
        labels={{
          removeValue: (entry) => t("tagPickerRemove", { entry }),
          invalidValue: (entry) => t("tagPickerInvalid", { entry }),
          maxValuesReached: (count) => t("tagPickerLimit", { count }),
        }}
      />
      {choices.length > 0 && (
        <div className="flex max-h-28 flex-wrap items-start gap-2 overflow-y-auto p-1">
          {choices.map((choice) => (
            <Button key={choice} type="button" size="sm" variant="secondary" className="max-w-full"
              disabled={disabled}
              onClick={() => {
                onChange(maxValues === 1 ? [choice] : [...value, choice]);
                input.current?.focus();
              }}>
              <span className="truncate">{choice}</span>
            </Button>
          ))}
        </div>
      )}
    </div>
  );
}
