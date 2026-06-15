import { useTranslation } from "react-i18next";

/// Stub for management areas not yet built into the new UI — the backend
/// endpoints exist; these screens are the next slices to add.
export function PlaceholderPage({ titleKey }: { titleKey: string }) {
  const { t } = useTranslation();
  return (
    <div>
      <h1 className="mb-2 text-2xl font-semibold">{t(titleKey)}</h1>
      <p className="text-color-muted">Coming soon.</p>
    </div>
  );
}
