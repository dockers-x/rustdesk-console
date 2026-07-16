# Admin Console Responsive UI/UX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver a responsive, restrained, and faster RustDesk admin console and publish it as `v0.2.29`.

**Architecture:** Separate mobile drawer visibility from desktop sidebar compaction, strengthen shared dialog and table layout contracts, and keep page-specific polish local to the login and overview screens. Move authenticated routes into a lazy-loaded module so public authentication routes do not eagerly download the whole console.

**Tech Stack:** React 19, TypeScript, React Router 6, Tailwind CSS 4, Cloudflare Kumo, Vite 6, Rust workspace/Cargo, agent-browser.

## Global Constraints

- Preserve backend APIs, database schemas, authorization, localization, Kumo components, and dark mode.
- Add no new dependency.
- Support 320, 390, 768, 1024, and 1440 pixel viewport widths.
- Mobile navigation must consume zero layout width when closed.
- Shared dialogs must fit inside 320px and 390px viewports.
- Resource data and row actions must remain reachable on narrow screens.
- High-frequency desktop navigation must not animate layout width.
- Motion uses transform/opacity, stays below 200ms, and honors `prefers-reduced-motion`.
- Preserve meaningful status colors, badges, avatars, progress bars, and monospace identifiers.
- Release version is exactly `0.2.29`; annotated tag is exactly `v0.2.29`.

---

## File Structure

- Modify `web/src/components/AppShell.tsx`: responsive shell state, mobile drawer behavior, touch targets, Escape handling.
- Modify `web/src/components/DialogLayout.tsx`: shared viewport-safe dialog sizing and scrolling.
- Modify `web/src/components/ConfirmDialog.tsx`: use compact shared dialog sizing.
- Modify `web/src/pages/MyProfilePage.tsx`: use compact shared dialog sizing.
- Modify `web/src/pages/ServerCommandsPage.tsx`: use compact shared dialog sizing for the base rules dialog.
- Modify `web/src/resource/ResourcePage.tsx`: responsive toolbar and horizontally operable semantic tables.
- Modify `web/src/i18n.ts`: localized accessible label for scrollable data tables.
- Modify `web/src/styles.css`: motion tokens, pointer press feedback, hover gating, reduced motion.
- Modify `web/src/components/PeerInfoDrawer.tsx`: remove pulsing online indicator.
- Modify `web/src/resource/registry.tsx`: remove pulsing device status indicator.
- Modify `web/src/pages/LoginPage.tsx`: title hierarchy, contextual labels, icon treatment, touch targets.
- Modify `web/src/pages/OverviewPage.tsx`: hierarchy, version metadata, and decorative icon reduction.
- Create `web/src/AuthenticatedApp.tsx`: lazy authenticated route tree.
- Modify `web/src/App.tsx`: public/auth route tree and lazy authenticated application boundary.
- Modify `Cargo.toml` and `Cargo.lock`: bump workspace crates to `0.2.29`.

---

### Task 1: Responsive Application Shell

**Files:**
- Modify: `web/src/components/AppShell.tsx`

**Interfaces:**
- Consumes: existing `NAV_SECTIONS`, `isItemActive`, React Router navigation, and i18n labels.
- Produces: `desktopCollapsed`, `mobileNavOpen`, and `isMobile` behavior used only inside `AppShell`.

- [ ] **Step 1: Record the current mobile failure**

Run the dev server and authenticate, then run:

```bash
agent-browser --session rustdesk-console-ui set viewport 390 844
agent-browser --session rustdesk-console-ui eval '({asideWidth: document.querySelector("aside")?.getBoundingClientRect().width, mainWidth: document.querySelector("main")?.getBoundingClientRect().width, viewport: window.innerWidth})'
```

Expected before implementation: closed navigation still reports `asideWidth: 64` and the content width is reduced.

- [ ] **Step 2: Split desktop and mobile navigation state**

Replace the current `collapsed` initialization and media-query effect with:

```tsx
const mobileQuery = "(max-width: 767px)";
const [desktopCollapsed, setDesktopCollapsed] = useState(false);
const [isMobile, setIsMobile] = useState(
  () => typeof window !== "undefined" && window.matchMedia(mobileQuery).matches,
);
const [mobileNavOpen, setMobileNavOpen] = useState(false);

useEffect(() => {
  const media = window.matchMedia(mobileQuery);
  const syncViewport = (matches: boolean) => {
    setIsMobile(matches);
    setMobileNavOpen(false);
  };
  syncViewport(media.matches);
  const onChange = (event: MediaQueryListEvent) => syncViewport(event.matches);
  media.addEventListener("change", onChange);
  return () => media.removeEventListener("change", onChange);
}, []);

useEffect(() => {
  if (!mobileNavOpen) return;
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Escape") setMobileNavOpen(false);
  };
  window.addEventListener("keydown", onKeyDown);
  return () => window.removeEventListener("keydown", onKeyDown);
}, [mobileNavOpen]);

const navCompact = !isMobile && desktopCollapsed;
const navExpanded = isMobile ? mobileNavOpen : !desktopCollapsed;
```

Change `closeMobileNav` to:

```tsx
const closeMobileNav = () => {
  if (isMobile) setMobileNavOpen(false);
};
```

- [ ] **Step 3: Render a zero-width closed mobile drawer**

Use `mobileNavOpen` for the backdrop and give the aside transform-based mobile positioning:

```tsx
{isMobile && mobileNavOpen && (
  <button
    type="button"
    className="fixed inset-0 z-30 bg-black/50"
    aria-label={t("collapseSidebar")}
    onClick={() => setMobileNavOpen(false)}
  />
)}
<aside
  className={cn(
    "fixed inset-y-0 left-0 z-40 flex shrink-0 flex-col border-r border-kumo-line bg-kumo-elevated",
    "transition-transform duration-[180ms] motion-reduce:transition-none",
    "[transition-timing-function:var(--ease-out)]",
    isMobile && !mobileNavOpen && "-translate-x-full pointer-events-none",
    isMobile && mobileNavOpen && "translate-x-0",
    !isMobile && "relative z-auto translate-x-0 transition-none",
  )}
  style={{ width: isMobile ? "min(280px, calc(100vw - 48px))" : navCompact ? 64 : 220 }}
>
```

Replace every presentation use of `collapsed` with `navCompact`, while section expansion remains controlled by `openSections`. On mobile, the expanded labels and section buttons are always rendered.

- [ ] **Step 4: Update the header controls and touch targets**

Use this toggle behavior:

```tsx
onClick={() => {
  if (isMobile) setMobileNavOpen((open) => !open);
  else setDesktopCollapsed((collapsed) => !collapsed);
}}
aria-expanded={navExpanded}
aria-label={navExpanded ? t("collapseSidebar") : t("expandSidebar")}
```

Add `className="min-h-11 min-w-11 justify-center md:min-h-0 md:min-w-0"` to the menu and theme controls, `min-h-11 md:min-h-0` to language/logout controls, `max-md:min-h-11` to drawer rows, and hide only the logout text below 360px while keeping the localized button label.

- [ ] **Step 5: Build and verify the shell**

Run:

```bash
cd web && pnpm run build
```

Expected: TypeScript and Vite build succeed.

Then verify at 390px:

```bash
agent-browser --session rustdesk-console-ui set viewport 390 844
agent-browser --session rustdesk-console-ui eval '({aside: document.querySelector("aside")?.getBoundingClientRect().toJSON(), mainWidth: document.querySelector("main")?.getBoundingClientRect().width, viewport: window.innerWidth})'
```

Expected: the closed aside is translated outside the viewport and `mainWidth` equals the viewport width. Open the drawer, press Escape, and confirm it closes.

- [ ] **Step 6: Commit the shell**

```bash
git add web/src/components/AppShell.tsx
git commit -m "feat: make admin navigation responsive"
```

---

### Task 2: Viewport-safe Shared Dialogs

**Files:**
- Modify: `web/src/components/DialogLayout.tsx`
- Modify: `web/src/components/ConfirmDialog.tsx`
- Modify: `web/src/pages/MyProfilePage.tsx`
- Modify: `web/src/pages/ServerCommandsPage.tsx`

**Interfaces:**
- Consumes: Kumo `Dialog` sizing classes.
- Produces: `dialogPanelClass`, `compactDialogPanelClass`, and `resourceFormDialogPanelClass` as complete flex-column dialog contracts.

- [ ] **Step 1: Record the current dialog overflow**

Open the system welcome preview at 390px and run:

```bash
agent-browser --session rustdesk-console-ui eval '({viewport: window.innerWidth, dialog: document.querySelector("[role=dialog]")?.getBoundingClientRect().toJSON()})'
```

Expected before implementation: `dialog.width` is about `512`, wider than the `390` viewport.

- [ ] **Step 2: Replace shared dialog class contracts**

Use explicit important width/min-width overrides because Kumo's `size` variants
set `min-width` values that otherwise exceed a mobile viewport:

```tsx
const dialogBaseClass =
  "z-50 flex max-h-[calc(100svh-1rem)] !min-w-0 flex-col overflow-hidden border border-kumo-line p-0 text-kumo-default shadow-lg ring-0 sm:max-h-[min(90vh,760px)]";

export const dialogPanelClass =
  `${dialogBaseClass} !w-[calc(100vw-1rem)] !max-w-[calc(100vw-1rem)] bg-kumo-elevated sm:!w-[min(32rem,calc(100vw-2rem))] sm:!max-w-[min(32rem,calc(100vw-2rem))]`;

export const compactDialogPanelClass =
  `${dialogBaseClass} !w-[calc(100vw-1rem)] !max-w-[calc(100vw-1rem)] bg-kumo-elevated sm:!w-[min(24rem,calc(100vw-2rem))] sm:!max-w-[min(24rem,calc(100vw-2rem))]`;

export const resourceFormDialogPanelClass =
  `${dialogBaseClass} !w-[calc(100vw-1rem)] !max-w-[calc(100vw-1rem)] bg-kumo-base sm:!w-[min(56rem,calc(100vw-2rem))] sm:!max-w-[min(56rem,calc(100vw-2rem))]`;
```

Keep `DialogHeader`, `DialogBody`, and `DialogFooter` fixed/scrolling responsibilities unchanged. Use `compactDialogPanelClass` in `ConfirmDialog`, the small avatar dialog in `MyProfilePage`, and the base-size rules dialog in `ServerCommandsPage`; keep `dialogPanelClass` on large dialogs.

- [ ] **Step 3: Build and verify dialog dimensions**

Run `cd web && pnpm run build` and repeat the 390px dialog measurement.

Expected: `dialog.left >= 8`, `dialog.right <= 382`, and `dialog.height <= 828`. Repeat at 320×568 and confirm the body scrolls while header/footer remain visible.

- [ ] **Step 4: Commit dialog fixes**

```bash
git add web/src/components/DialogLayout.tsx web/src/components/ConfirmDialog.tsx web/src/pages/MyProfilePage.tsx web/src/pages/ServerCommandsPage.tsx
git commit -m "fix: keep admin dialogs inside mobile viewports"
```

---

### Task 3: Responsive Resource Toolbars and Tables

**Files:**
- Modify: `web/src/resource/ResourcePage.tsx`
- Modify: `web/src/i18n.ts`

**Interfaces:**
- Consumes: Kumo `Table` className support and existing resource configs.
- Produces: localized `scrollableTable` label and focusable horizontal table regions.

- [ ] **Step 1: Record the current table compression**

Open `#/users` at 390px and run:

```bash
agent-browser --session rustdesk-console-ui eval '(() => { const table = document.querySelector("table"); const wrap = table?.parentElement; return {viewport: innerWidth, clientWidth: wrap?.clientWidth, scrollWidth: wrap?.scrollWidth, headerHeights: [...document.querySelectorAll("th")].map((node) => node.getBoundingClientRect().height)}; })()'
```

Expected before implementation: headings wrap to multiple lines and the table is compressed to the wrapper width.

- [ ] **Step 2: Add the accessible scroll-region label**

Add to the English dictionary near `actions`:

```ts
scrollableTable: "Scrollable data table",
```

Add to the Chinese dictionary near `actions`:

```ts
scrollableTable: "可横向滚动的数据表格",
```

- [ ] **Step 3: Make filters and tables narrow-screen safe**

Change the toolbar action container to `w-full sm:w-auto`, make text filters `className="min-w-0 flex-1 sm:min-w-48 sm:flex-none"`, selects `w-full sm:w-auto`, and the create button `className="shrink-0"`.

Replace the table wrapper/root with:

```tsx
<div
  className="min-w-0 overflow-x-auto overscroll-x-contain rounded-lg border border-kumo-line focus:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand"
  role="region"
  aria-label={t("scrollableTable")}
  tabIndex={0}
>
  <Table className="min-w-max [&_th]:whitespace-nowrap [&_td]:whitespace-nowrap [&_td]:align-top">
```

Keep all configured columns and actions. Do not hide data on mobile.

- [ ] **Step 4: Build and verify table reachability**

Run `cd web && pnpm run build`.

At 390px, repeat the measurement. Expected: `scrollWidth > clientWidth`, table headers remain one line, the page itself has no horizontal overflow, and horizontal scrolling reaches the action column. Tab to the region and confirm the focus ring is visible.

- [ ] **Step 5: Commit resource responsiveness**

```bash
git add web/src/resource/ResourcePage.tsx web/src/i18n.ts
git commit -m "fix: improve admin tables on narrow screens"
```

---

### Task 4: Motion Tokens and Status Feedback

**Files:**
- Modify: `web/src/styles.css`
- Modify: `web/src/components/PeerInfoDrawer.tsx`
- Modify: `web/src/resource/registry.tsx`

**Interfaces:**
- Produces CSS variables `--ease-out` and `--ease-in-out` for application motion.

- [ ] **Step 1: Add shared motion rules**

Append:

```css
:root {
  --ease-out: cubic-bezier(0.23, 1, 0.32, 1);
  --ease-in-out: cubic-bezier(0.77, 0, 0.175, 1);
}

@media (hover: hover) and (pointer: fine) {
  button:not(:disabled) {
    transition-property: transform, background-color, color, border-color, opacity;
    transition-duration: 140ms;
    transition-timing-function: var(--ease-out);
  }

  button:not(:disabled):active:not(:focus-visible) {
    transform: scale(0.98);
  }
}

@media (prefers-reduced-motion: reduce) {
  html {
    scroll-behavior: auto;
  }

  button:not(:disabled) {
    transition-property: background-color, color, border-color, opacity;
    transition-duration: 80ms;
  }

  button:not(:disabled):active {
    transform: none;
  }

  [role="dialog"] {
    scale: 1 !important;
    transition-property: opacity !important;
    transition-duration: 80ms !important;
  }
}
```

- [ ] **Step 2: Remove pulsing device indicators**

In both `PeerInfoDrawer.tsx` and `registry.tsx`, replace the nested ping structure:

```tsx
<span className="relative flex size-2.5">
  <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-kumo-success opacity-60 motion-reduce:hidden" />
  <span className="relative inline-flex size-2.5 rounded-full bg-kumo-success" />
</span>
```

with:

```tsx
<span className="inline-flex size-2.5 rounded-full bg-kumo-success" aria-hidden="true" />
```

- [ ] **Step 3: Build and scan motion code**

Run:

```bash
cd web && pnpm run build
rg -n "animate-ping|transition-all|hover:scale-105|ease-in\b" src
```

Expected: build passes; no `animate-ping`, `transition-all`, or decorative hover scaling remains in application source.

- [ ] **Step 4: Commit motion/status cleanup**

```bash
git add web/src/styles.css web/src/components/PeerInfoDrawer.tsx web/src/resource/registry.tsx
git commit -m "feat: refine admin motion and status feedback"
```

---

### Task 5: Login and Overview Hierarchy

**Files:**
- Modify: `web/src/pages/LoginPage.tsx`
- Modify: `web/src/pages/OverviewPage.tsx`

**Interfaces:**
- Preserves: all login/setup/reset/OAuth/captcha behavior and all overview data fields.

- [ ] **Step 1: Refine the login presentation**

In `AuthSignal`, remove the rounded icon tile and render the icon directly:

```tsx
<Icon
  size={18}
  weight={active ? "fill" : "regular"}
  className={active ? "text-kumo-default" : "text-kumo-subtle"}
  aria-hidden
/>
```

Replace the top pill with a quiet label:

```tsx
<div className="inline-flex items-center gap-2 text-xs font-medium text-kumo-subtle">
  <Monitor size={16} aria-hidden />
  <span>{t("loginSurfaceTag")}</span>
</div>
```

Change the title classes to:

```tsx
className="mt-5 max-w-xl break-words text-3xl font-semibold leading-tight sm:text-4xl lg:text-4xl"
```

Change the form context label from uppercase styling to:

```tsx
className="flex min-h-6 items-center gap-2 text-xs font-medium text-kumo-subtle"
```

Add `min-h-11` to login/setup/reset inputs, full-width auth buttons, captcha refresh, and registration link; keep desktop behavior otherwise unchanged.

- [ ] **Step 2: Refine overview visual weight**

Remove the `Badge` import and render version metadata as:

```tsx
<span className="text-xs tabular-nums text-kumo-subtle">
  {t("version")} {data.version}
</span>
```

In section headers, remove the bordered icon tile, render the icon directly with `className="mt-0.5 shrink-0 text-kumo-brand"`, and change `h2` to `text-lg font-semibold`.

In `MetricCard`, replace the bordered tile with:

```tsx
<div className="shrink-0 text-kumo-brand" aria-hidden="true">
  {icon}
</div>
```

Add an optional `secondary?: boolean` prop and use `bg-kumo-base` plus a slightly smaller value size for the lower four metrics. Pass `secondary` to login logs, file audit, share records, and uptime cards.

- [ ] **Step 3: Build and visually verify**

Run `cd web && pnpm run build`.

Capture login and overview at 390×844 and 1440×900 in both themes. Expected: the default title is not forced to split on desktop, touch targets are at least 44px on mobile, overview primary metrics remain the first scan target, and real status/content remains visible.

- [ ] **Step 4: Commit page hierarchy changes**

```bash
git add web/src/pages/LoginPage.tsx web/src/pages/OverviewPage.tsx
git commit -m "feat: clarify admin login and overview hierarchy"
```

---

### Task 6: Lazy-load the Authenticated Console

**Files:**
- Create: `web/src/AuthenticatedApp.tsx`
- Modify: `web/src/App.tsx`

**Interfaces:**
- `AuthenticatedApp` default export renders the existing protected route tree.
- `App` retains public routes and `RequireAuth`, then lazy-loads `AuthenticatedApp` for `/*`.

- [ ] **Step 1: Capture the current single-chunk build**

Run:

```bash
cd web && pnpm run build
find ../resources/admin/assets -maxdepth 1 -type f -printf '%f %s\n' | sort
```

Expected before implementation: one main JavaScript chunk is roughly 1.14MB before gzip.

- [ ] **Step 2: Create the authenticated route module**

Create `web/src/AuthenticatedApp.tsx` containing the imports currently needed only after authentication and this route tree:

```tsx
import { Navigate, Route, Routes } from "react-router-dom";
import { AppShell } from "./components/AppShell";
import { DiagnosticsPage } from "./pages/DiagnosticsPage";
import { MessageCenterPage } from "./pages/MessageCenterPage";
import { MyProfilePage } from "./pages/MyProfilePage";
import { NotificationRoutingPage } from "./pages/NotificationRoutingPage";
import { OAuthActionPage } from "./pages/OAuthActionPage";
import { OverviewPage } from "./pages/OverviewPage";
import { ServerCommandsPage } from "./pages/ServerCommandsPage";
import { SystemSettingsPage } from "./pages/SystemSettingsPage";
import { WebClientSettingsPage } from "./pages/WebClientSettingsPage";
import { ResourcePage } from "./resource/ResourcePage";
import { ALL_RESOURCES, resourcePath } from "./resource/registry";

const HOME = "/overview";

export default function AuthenticatedApp() {
  return (
    <Routes>
      <Route element={<AppShell />}>
        <Route path="/" element={<Navigate to={HOME} replace />} />
        <Route path="/overview" element={<OverviewPage />} />
        <Route path="/diagnostics" element={<DiagnosticsPage />} />
        <Route path="/my" element={<MyProfilePage />} />
        <Route path="/messages" element={<MessageCenterPage />} />
        <Route path="/notification-routing" element={<NotificationRoutingPage />} />
        <Route path="/settings" element={<SystemSettingsPage />} />
        <Route path="/serverCmd" element={<ServerCommandsPage />} />
        <Route path="/webclient-settings" element={<WebClientSettingsPage />} />
        <Route path="/oauth/:code" element={<OAuthActionPage mode="confirm" />} />
        <Route path="/oauth/bind/:code" element={<OAuthActionPage mode="bind" />} />
        {ALL_RESOURCES.map((resource) => (
          <Route
            key={resource.name}
            path={resourcePath(resource)}
            element={<ResourcePage cfg={resource} />}
          />
        ))}
      </Route>
      <Route path="*" element={<Navigate to={HOME} replace />} />
    </Routes>
  );
}
```

- [ ] **Step 3: Reduce `App.tsx` to public/auth routes plus a lazy boundary**

Keep `RequireAuth`, `LoginPage`, `RegisterPage`, `ForceChangePasswordPage`, and `AppTitleController`. Add:

```tsx
import { lazy, Suspense } from "react";
import { useTranslation } from "react-i18next";

const AuthenticatedApp = lazy(() => import("./AuthenticatedApp"));

function RouteLoading() {
  const { t } = useTranslation();
  return (
    <div className="flex min-h-full items-center justify-center bg-kumo-base p-6 text-sm text-kumo-subtle" role="status">
      {t("loading")}
    </div>
  );
}
```

Replace the protected nested route tree with:

```tsx
<Route
  path="/*"
  element={
    <RequireAuth>
      <Suspense fallback={<RouteLoading />}>
        <AuthenticatedApp />
      </Suspense>
    </RequireAuth>
  }
/>
```

Keep `/change-password` before this wildcard and remove authenticated-only imports from `App.tsx`.

- [ ] **Step 4: Build and verify chunk separation**

Run:

```bash
cd web && pnpm run build
find ../resources/admin/assets -maxdepth 1 -type f -printf '%f %s\n' | sort
```

Expected: at least two JavaScript chunks, with the public entry materially smaller than the previous 1.14MB main chunk. Verify unauthenticated `/login`, authenticated `/overview`, direct `#/users`, and `/change-password` routing.

- [ ] **Step 5: Commit route splitting**

```bash
git add web/src/App.tsx web/src/AuthenticatedApp.tsx
git commit -m "perf: lazy load authenticated admin routes"
```

---

### Task 7: Full Verification, Version Bump, Tag, and Push

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`

**Interfaces:**
- Produces workspace version `0.2.29` and annotated Git tag `v0.2.29`.

- [ ] **Step 1: Run frontend and Rust verification before versioning**

Run:

```bash
cd web && pnpm run build
cd .. && cargo test --workspace
cargo build --workspace
node /home/czyt/.cc-switch/skills/skill/scripts/scan.mjs web/src
git diff --check
```

Expected: all builds/tests pass, the scanner no longer reports pulsing status dots, and remaining scanner hits are manually classified as intentional or false positives.

- [ ] **Step 2: Run the responsive browser matrix**

Using the local server and `agent-browser`, check light/dark themes at:

```text
320×568
390×844
768×1024
1024×768
1440×900
```

At each applicable size verify login, sidebar/drawer, overview, users table, system welcome dialog, keyboard focus, and no document-level horizontal overflow. At 390px verify Escape closes the drawer; with reduced motion enabled verify positional animations are effectively removed.

### Release review amendment

- Make authentication storage observable so a 403-cleared token immediately
  rerenders the route guard and redirects to login.
- Add short page-entry, overview-stagger, and mobile-backdrop motion using only
  transform and opacity, with all movement disabled by reduced motion.
- Verify an invalid stored token redirects from `#/overview` to `#/login`, and
  confirm the new animations stay at or below 220ms.

- [ ] **Step 3: Bump the workspace version**

Run:

```bash
cargo set-version --workspace 0.2.29
```

If `cargo-set-version` is unavailable, edit the root `[workspace.package] version` to `0.2.29`, then run:

```bash
cargo check --workspace
```

Expected: `Cargo.toml` and all workspace package entries in `Cargo.lock` report `0.2.29`.

- [ ] **Step 4: Re-run release verification**

Run:

```bash
cd web && pnpm run build
cd .. && cargo test --workspace
cargo build --workspace
git diff --check
git status --short
```

Expected: all commands pass; only intentional source, plan/spec, and version changes are present.

- [ ] **Step 5: Commit the version bump**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: bump version to 0.2.29"
```

- [ ] **Step 6: Review the final history and create the annotated tag**

Run:

```bash
git log --oneline --decorate -8
git status --short
git tag -a v0.2.29 -m "v0.2.29

- Make the admin shell, dialogs, and tables responsive across mobile and desktop sizes.
- Refine login, overview, motion, device status, and expired-session handling.
- Lazy-load authenticated routes to reduce the login-page payload.
- Bump workspace package version to 0.2.29."
```

Expected: clean working tree and tag `v0.2.29` points to the version commit.

- [ ] **Step 7: Push main and the release tag**

Run:

```bash
git push origin main
git push origin v0.2.29
```

Expected: both pushes succeed. Verify with:

```bash
git ls-remote --heads origin main
git ls-remote --tags origin refs/tags/v0.2.29 refs/tags/v0.2.29^{}
```

The remote main hash must equal local `HEAD`, and the dereferenced annotated tag must resolve to the same release commit.
