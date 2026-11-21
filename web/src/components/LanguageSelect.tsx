import { Translate } from "@phosphor-icons/react";
import { useTranslation } from "react-i18next";

const languages = [
  { value: "en", label: "English" },
  { value: "ru", label: "Русский" },
  { value: "zh-CN", label: "简体中文" },
];

export function LanguageSelect() {
  const { t, i18n } = useTranslation();
  return (
    <label className="relative inline-flex shrink-0 items-center text-sm">
      <Translate size={16} aria-hidden className="pointer-events-none absolute left-2 text-kumo-subtle" />
      <select
        aria-label={t("interfaceLanguage")}
        className="min-h-11 cursor-pointer rounded-md bg-transparent pl-8 pr-1 text-kumo-default hover:bg-kumo-tint focus-visible:outline-2 focus-visible:outline-kumo-brand md:min-h-8"
        value={i18n.language}
        onChange={(event) => {
          localStorage.setItem("lang", event.target.value);
          void i18n.changeLanguage(event.target.value);
        }}
      >
        {languages.map(({ value, label }) => <option key={value} lang={value} value={value}>{label}</option>)}
      </select>
    </label>
  );
}
