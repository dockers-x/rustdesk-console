import { renderToStaticMarkup } from "react-dom/server";
import { expect, it, vi } from "vitest";
import { emptyStorageConfig } from "../lib/storageProfiles";
import { StorageProfileFields } from "./StorageProfileFields";

vi.mock("../lib/api", () => ({ apiGet: vi.fn() }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

it("shows all three storage backends as choices before the user interacts", () => {
  const form = renderToStaticMarkup(
    <StorageProfileFields config={emptyStorageConfig} onChange={() => {}} locked={false} />,
  );
  expect(form.match(/role="radio"/g)).toHaveLength(3);
  for (const label of ["recordStorageLocal", "S3", "WebDAV"]) {
    expect(form).toContain(`>${label}<`);
  }
});
