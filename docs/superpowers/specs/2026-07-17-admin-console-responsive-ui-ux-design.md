# Admin Console Responsive UI/UX Design

Date: 2026-07-17
Target release: `v0.2.29`

## Summary

Refine the existing RustDesk admin console without replacing its visual system.
The work focuses on responsive navigation, mobile-safe dialogs and data tables,
touch ergonomics, clearer dashboard hierarchy, restrained motion, and a faster
login-page payload. It also removes a small set of confirmed template-like
decorations while preserving deliberate RustDesk and Kumo design choices.

## Goals

- Make the console usable at 320, 390, 768, 1024, and 1440 pixel viewport widths.
- Give mobile content the full viewport width while keeping navigation easy to
  discover and operate.
- Ensure every shared dialog stays inside the viewport and scrolls internally.
- Keep resource tables complete and operable on narrow screens.
- Improve touch targets without reducing desktop information density.
- Clarify the information hierarchy on the login and overview pages.
- Remove motion and decoration that do not communicate state or spatial change.
- Reduce the JavaScript required before the login screen becomes interactive.
- Preserve backend behavior, API contracts, Kumo components, dark mode, and
  localization.

## Non-goals

- Rebranding RustDesk or replacing the Kumo component library.
- Redesigning every resource-specific form or rewriting all copy.
- Removing legitimate status colors, badges, avatars, progress bars, or
  monospace formatting for code and identifiers.
- Changing backend routes, database schemas, authorization, or business logic.
- Building a separate mobile application.

## Design Principles

1. Optimize task completion before visual novelty.
2. Preserve intentional choices; remove only decoration without information.
3. Use hierarchy from size, spacing, and alignment rather than extra containers.
4. Keep high-frequency desktop actions immediate.
5. Animate only transform and opacity when motion is necessary.
6. Treat narrow screens as a first-class layout, not a compressed desktop view.

## Responsive Application Shell

### State model

The current `collapsed` state controls both desktop compaction and mobile
visibility. These are separate behaviors and will become separate state:

- `desktopCollapsed`: chooses between the 64px icon rail and 220px expanded
  sidebar on viewports at or above the `md` breakpoint.
- `mobileNavOpen`: controls whether the expanded navigation drawer is visible
  below the `md` breakpoint.
- `isMobile`: follows `matchMedia("(max-width: 767px)")` and closes the drawer
  when the viewport crosses modes.

On mobile, a closed drawer occupies no layout width. Opening it shows an
expanded 220–280px navigation surface over a backdrop. Selecting a destination,
clicking the backdrop, or pressing Escape closes it.

On desktop, collapsing the sidebar is immediate. The layout does not animate
`width`, because that triggers repeated layout work and makes a frequent action
feel slower. The occasional mobile drawer uses a 180ms transform transition with
a strong ease-out curve.

### Header and navigation controls

- Header controls and mobile navigation rows have a minimum 44px touch target.
- The logout label may hide at the narrowest width while the icon keeps an
  accessible name and tooltip.
- The desktop rail keeps existing icons and native titles.
- Focus rings remain visible in both themes.

## Shared Dialogs

`dialogPanelClass` becomes a complete responsive panel contract rather than only
a surface style:

- width: `calc(100vw - 1rem)` on mobile;
- maximum width: `calc(100vw - 1rem)` on mobile and the Kumo size limit above
  `sm`;
- maximum height: `calc(100svh - 1rem)` on mobile and a restrained desktop cap;
- flex column layout with a fixed header/footer and scrolling body;
- solid surface, hairline edge, and restrained elevation;
- centered transform origin because dialogs are viewport-level surfaces.

The existing resource form dialog adopts the same shared sizing principles.
Dialog enter/exit motion remains below 200ms and falls back to opacity-only when
the user requests reduced motion.

## Resource Tables

Resource pages continue to use semantic tables and show every configured column.
A generic card conversion is intentionally avoided because resource definitions
vary widely and hiding columns would silently remove administrative information.

For narrow viewports:

- the table receives a content-driven minimum width;
- headings and compact values avoid destructive character-by-character wrapping;
- the existing wrapper remains horizontally scrollable by touch and keyboard;
- scrolling is contained to the table region;
- filter and create controls wrap without overflowing;
- pagination remains reachable beneath the table.

This is a conservative, shared fix that improves every resource page without
introducing per-resource mobile schemas.

## Login Experience

The login page keeps its subtle grid background because it is a restrained,
product-appropriate technical motif. The main surface and authentication status
summary also remain.

Targeted changes:

- allow the configurable console title to wrap naturally instead of forcing a
  short measure that splits the default title;
- reduce the largest desktop title size to preserve balance with the form;
- convert repeated uppercase pill-like labels into quiet contextual labels;
- remove bordered rounded-square containers around authentication summary icons;
- retain flat status dots because they communicate actual availability;
- raise input and action touch targets to 44px on touch layouts;
- keep setup, password reset, captcha, OAuth, and verification behavior intact.

## Overview Hierarchy

The overview remains a dashboard rather than being flattened into prose.
Changes are limited to hierarchy and visual weight:

- keep the four primary operational metrics as the first scan target;
- reduce decorative icon containers and use icons as secondary labels;
- make section headings visibly distinct from body text;
- render the real version as low-emphasis metadata rather than the strongest
  badge on the page;
- visually subordinate secondary counters to the primary metrics;
- preserve platform distribution and recent-activity tables.

No metrics are invented, removed, or combined in ways that change their meaning.

## Status and Feedback

- Remove `animate-ping` from online device indicators in the peer drawer and
  resource registry.
- Keep a flat dot and explicit status text.
- Keep semantic error and success messages because they communicate real state;
  do not neutralize them merely to reduce scanner findings.
- Preserve loading, empty, and error roles used by assistive technology.

## Motion System

Add shared motion tokens:

- strong ease-out: `cubic-bezier(0.23, 1, 0.32, 1)`;
- movement ease-in-out: `cubic-bezier(0.77, 0, 0.175, 1)`;
- press feedback: 120–160ms;
- popovers and dialogs: 150–200ms;
- mobile navigation drawer: 180ms.

Pointer presses receive a subtle `scale(0.98)` only when focus is not
keyboard-visible. Keyboard-driven actions stay immediate. Hover effects are
gated to devices with hover and a fine pointer. `prefers-reduced-motion` removes
position and scale motion while retaining short opacity and color feedback.

### Motion enhancement amendment

Use motion at three visible boundaries without turning navigation into a show:

- authenticated page content enters over 160ms with 4px of vertical travel;
- the four primary overview metrics and two overview panels enter over 220ms
  with 35ms spacing between the metric cards;
- the mobile navigation backdrop fades in over 160ms while the drawer keeps its
  existing 180ms spatial transition.

All additions animate only transform and opacity. Reduced-motion mode removes
the page, overview, backdrop, drawer, and press movement.

Authentication state is reactive within the current tab and across storage
events. When an API response clears an invalid token, the route guard rerenders
and replaces the protected route with `/login`, preserving the attempted path
for the post-login return.

## Initial Load Performance

The login route currently imports the authenticated console and produces a
single JavaScript chunk of roughly 1.14MB before gzip. Separate public/auth
routes from authenticated application routes with `React.lazy` and Suspense:

- login, registration, OAuth confirmation, and forced-password flows stay in
  the public/auth entry path;
- the application shell, resource registry, dashboard, settings, and admin
  pages load after authentication or when their route is requested;
- the loading fallback uses the existing neutral status presentation.

This changes module loading only. Routing URLs, authentication checks, and API
calls remain the same.

## Accessibility and Localization

- Preserve the skip link, semantic navigation, table, dialog, status, and alert
  roles.
- Keep visible focus states and add Escape handling for the mobile drawer.
- Controls represented by icons retain localized accessible labels.
- No new table-scroll copy is introduced; the scroll region remains keyboard
  focusable and exposes a localized accessible label.
- Verify long Chinese and English labels at all target widths and at 200% zoom.

## Data Flow and Error Handling

The work does not alter server communication. React Query keys, mutations,
pagination, filters, authentication storage, and error propagation remain
unchanged. Layout changes wrap the existing rendering paths instead of creating
parallel data flows.

Lazy route loading does not change API retry semantics. Existing inline and
table error states continue to display server messages.

## Verification

### Automated

- `pnpm run build`.
- TypeScript project build through the existing web build script.
- `cargo test --workspace`.
- `cargo build --workspace` or the release-relevant server build.
- Re-run the `kill-ai-slop` scanner and manually classify remaining hits.
- Confirm the production build creates separate public/auth and authenticated
  chunks and no new dependency is introduced.

### Browser

Check light and dark themes at 320×568, 390×844, 768×1024, 1024×768, and
1440×900:

- login, setup-capable layout, and authentication form;
- mobile drawer open/close, route selection, backdrop, and Escape;
- overview hierarchy and metric wrapping;
- a wide resource table with filters, pagination, and row actions;
- shared small and large dialogs, including the system welcome preview;
- keyboard traversal and visible focus;
- reduced-motion behavior;
- 200% browser zoom without horizontal page overflow.

## Acceptance Criteria

- A closed mobile navigation drawer consumes zero content width.
- No shared dialog exceeds the viewport at 320 or 390 pixels.
- Resource data and row actions remain reachable on narrow screens.
- No top-level page introduces horizontal document scrolling at the target sizes.
- Mobile header controls, drawer rows, authentication controls, and primary form
  actions are at least 44px high.
- Online indicators no longer pulse.
- High-frequency desktop navigation does not animate layout width.
- The default login title is not artificially split by a fixed character measure.
- The login route no longer requires the complete authenticated admin bundle.
- Light/dark themes, localization, authentication, and backend behavior remain
  functional.

## Release

After verification:

1. Update the workspace version from `0.2.28` to `0.2.29` and refresh
   `Cargo.lock` through Cargo tooling.
2. Commit implementation and generated version changes with a focused message.
3. Create annotated tag `v0.2.29` summarizing responsive navigation, dialog and
   table fixes, UI hierarchy refinement, and login performance.
4. Push `main` and `v0.2.29` to `origin`.
