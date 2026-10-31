# ScreenTranslate design system

## Direction: Soft Utility / 柔和工具界面

ScreenTranslate is a compact desktop instrument for reading across languages. The user approved the centered About preview inspired by FlClash: soft grey-lilac backgrounds, opaque tonal surfaces, large rounded groups, circular icon wells and pill actions. Extend that visual language across the existing Svelte application. Use alignment, spacing and surface contrast for hierarchy, with minimal borders and no ornamental shadows. Retain the existing application icon, provider artwork and compact native window sizes.

## Principles

- Give screenshots and text enough space to read. Screenshot images are drag surfaces: no selectable text, click-to-read hotspots or reading button. Extract recognized source text through an explicit toolbar action; correction remains in the main workspace.
- Use one obvious primary action per surface. The main workspace has no shared page-title/search/capture header or sidebar brand row. Keep screenshot translation accessible in shortcuts, the capture workspace and the command palette.
- Use grouped rows and separators for preferences and connections. Avoid promotional cards, repeated captions and decorative gradients or glass.
- Separate inspect, select and apply. Looking at another OCR service must not silently enable it.
- Keep native main-window decorations, transparency, screen coordinates and ready/presented handshakes intact.

## Source of truth

`apps/desktop/src/design-tokens.css` owns theme and size tokens. `app.css` imports shared `controls.css` and main-layout `workspaces.css`; `windows.css` owns quick, selection, result, and modal surfaces. Import them once from `main.ts`. Do not recreate old per-page override layers or append a second style for an existing selector. Utility classes in the existing shadcn components remain available; application visual rules belong to semantic classes and tokens.

## Colour

| Token | Light | Dark |
| --- | --- | --- |
| background | #f0eef1 | #1d1c20 |
| sidebar | #e9e6ec | #211f25 |
| surface | #fbf9fc | #2a282e |
| muted surface | #ebe7ef | #35323b |
| foreground | #202127 | #f0edf3 |
| secondary | #56515e | #c3bdcc |
| muted text | #68636f | #b0a9ba |
| border | #ded9e3 | #45404c |
| brand | #4b4459 | #d4ccdf |
| primary action | #303030 | #f0f0f0 |
| primary foreground | #ffffff | #000000 |
| danger | #b53232 | #f5a3a0 |
| success | #357153 | #a0d1ae |

Brand colour is reserved for identity and primary emphasis. User accent colours override action colours, not service brand marks. Always compute a contrasting `--primary-foreground` and a visible focus colour together with `--primary`; never retain a foreground from an earlier theme. Test a pale accent, a saturated accent, both themes and a theme change.

## Typography and spacing

- Use installed system fonts with Chinese and macOS fallbacks; do not download fonts.
- Root: `14px * --font-scale`. UI body 1rem; secondary .9286rem; reading text 1.2143rem; page title 1.1429rem; dialog title 1.4286rem. All interface text participates in scaling. Screenshot overlay text follows the image's pixel scale and provides a separate reading alternative.
- Reading line height 1.7; controls and short labels 1.45. Avoid letter spacing on Chinese labels.
- Space grid: 4, 8, 12, 16, 20, 24, 32px. Main insets 16px; editor text insets 20px. Align all toolbar, panel and footer edges.
- Sidebar: 152px expanded / 56px collapsed. Preserve a stable icon column and toggle position.

## Components

- Default control height: at least 32px and 2.2857rem. Compact controls at least 28px and 2rem. Textareas and menus grow with the font scale.
- Radius: fields 16px; grouped surfaces and dialogs 24px; action buttons and tabs use the shared pill radius. Supporting toolbar buttons use the shared compact radius. Circular icon wells identify navigation/actions; do not add them to every input label. Do not assign arbitrary radii per page.
- Borders are 1px. Shadows belong to floating surfaces, menus and dialogs, not every row or content group.
- Hover changes surface/colour; pressed uses a short colour response and at most .98 scale, with no change to surrounding layout. Focus-visible uses a consistent 2px ring. Continuous text editors have no persistent border or pane line: editable areas use --editor-surface, reading areas use --reading-surface, and keyboard focus changes the editable surface to --editor-active while retaining the caret; structural Dialog containers do not get an extra ring. Disabled controls remain identifiable and cannot be activated.
- Reuse shadcn-svelte/Bits UI Button, Select, Switch, Tooltip, Dialog, Command and Resizable. Product-specific status and layout wrappers are acceptable; do not build replacements for primitives.

## Icons and motion

- Use the shared rounded single-line glyphs in lib/components/icons: 24px optical grid, 1.8-unit stroke, round caps and joins, 20px toolbar and navigation. Use open, simple silhouettes and rounded enclosures rather than dense keyboard/dotted detail. The S outline is the screenshot toolbar handle. Phosphor remains available for auxiliary glyphs and the loader; match its optical weight. Use Check/ArrowsLeftRight rather than unicode substitutes. Every icon button needs an accessible label and useful hover text.
- Hover/pressed and menus: 140ms; standard feedback: 180ms; layout: 220ms; sidebar width: 260ms. Module pages switch directly at a fixed origin without translate or zero-opacity entry; service details may use a short .8-to-1 opacity response; dialog: 160ms, <=.98 scale. Easing `cubic-bezier(.2,0,0,1)`. Navigation/tab selection uses the existing Svelte Spring marker; no continuous animation while idle.
- Use Svelte transition/motion or CSS transitions. No new animation dependency. No idle decorative animation or animation of native window coordinates. Sidebar width and main content resize together; keep the icon column stationary, fade labels on collapse, and respect reduced motion.
- Respect `prefers-reduced-motion` in both JS and CSS. Never delay window-ready, image readiness or presented signals for visual effects.

## Responsive and state rules

- Preserve the existing native default and minimum sizes. Verify main 780×520 and 600×400, quick 600×320 and 420×260, 150% text, and long translations.
- Text panes stack when their usable width is insufficient. At minimum sizes with large fonts, use source/translation tabs; quick tools use the same fallback. Correction always opens as a Dialog with fixed heading and scrolling reading panes, preserving the image preview underneath.
- Screenshot frames use a 2px blue outline with 6px white circular anchors. Keep the outline and all anchors visible while moving the image. Hide only the independent toolbar during image movement and resizing; after release reattach it to the image and avoid screen edges, even if it was previously detached. Dragging a side changes only that dimension and fixes the opposite side; corners change both dimensions independently. The image stage follows its native viewport directly, retaining 8px padding, the 48px reservation and the narrow-image allowance. Fit floating translations once in source-image coordinates, then scale the whole text layer on X/Y; do not repeatedly reflow or refit glyphs on every resize frame. The screenshot itself remains a drag surface with no clickable reading badge. The toolbar provides extraction/copy and main-window correction. Screenshot feedback appears in a centered compact pill for 2.6 seconds; keep full error details and recovery on the toolbar. For a tiny capture use the existing window center and temporarily quiet the toolbar, keeping the image bounds fixed. Do not add a large persistent warning panel.
- First-use guidance is a compact 360px Dialog with the small S outline wordmark as its accessible title; do not add a separate headline or introductory paragraph. Center original and translated text in the same comparison frame, clip them to opposite sides of a draggable vertical divider, and start at 50%. Use a native range input for pointer, touch and keyboard control; keep a visible focus indicator on the frame and do not animate the divider while dragging. Keep one primary start action, a quiet service configuration action and a brief but explicit local/cloud OCR label. At short window heights make the comparison frame shorter so it remains available at 150% text. Reuse Bits Dialog for focus, Escape and dismissal; the initial dialog container has no decorative focus ring.
- Show errors next to the affected work, with a specific recovery action. Avoid showing the same error as both a panel and a footer message. Saving feedback is quiet; preserve failed values and retry.
- Restore normal context menus in text fields. Keyboard hints must match implemented handlers, and handlers must ignore input, composition, menus and dialogs where appropriate.

## Consistency and verification

All windows share colour, type, focus, radius and button tokens. Capture selection may retain a high-visibility blue outline as an explicit tool semantic; it is not the application brand colour. Keep overlay padding 8px and legacy controls reservation 48px to preserve native readiness and compact-notice geometry. The desktop toolbar is a separate transparent native window, never a CSS-translated child clipped by the screenshot. Dragging S detaches it; double-click or arrow keys dock it again. Attached tools follow the image and avoid monitor work-area edges automatically; detached tools remain freely positioned and clamp to the work area. Store explicit docking preference separately from automatic edge avoidance: return below the image when there is room again unless the user explicitly chose the top. Docking does not alter the image's physical origin. Hide and restore both windows together, and keep the toolbar above the image on Windows. Source and translation copy buttons have independent success indicators and timers. Main text/result workspaces have no duplicate progress footer; retain processing and recovery feedback with the affected content and controls. Theme previews win over older autosave replies and must not remount the settings page. Selected navigation text has no colour transition; its shared neutral selection surface follows the chosen row. Suppress other transitions briefly while applying theme tokens, and on Windows update DWM caption attributes without resetting the WebView theme.

After each related window/component group: run `pnpm check`, inspect actual Svelte rendering, record at least five visible issues and targeted corrections. Keep screenshots of important windows and failure/minimum-size cases. Browser IPC mocks must stay outside shipped source and be labelled as previews; they do not verify native chrome, DPI, OS capture, OCR or global shortcuts. Finish with existing tests, production build and bundle budget checks. Record actual results only.

## Reading and service navigation

- Screenshot workspaces have no mosaic, text annotation, annotation undo or export controls. Preserve the original image and OCR/correction workflow.
- Correction is one wide in-app Dialog, never an additional native window. Put target language and retranslation in its fixed header; show original and translation in equal reading columns with independent scroll. Use stacked panes or tabs when width and font scaling require it. Editable reading panes use surface contrast without outlines; keyboard focus changes their surface rather than adding a line. Read-only keyboard-focusable panes retain an inset focus indicator. Mark an edited original as pending until a matching translation returns.
- Text translation has one translate action at the top right, replacing the engine chip. While busy, the same position offers cancellation. Do not leave a bottom action row.
- Services use one directory at the left and a stable detail pane at the right, under the existing translation/OCR tabs. Group translation services as API services and free interfaces; browse API services by manufacturer rather than exposing generic/dedicated protocol categories. Replace the directory with a service select at narrow sizes; never shrink the form until its fields overflow. The quiet tonal configuration surface scrolls separately from its fixed heading and actions. Selecting a service does not enable it; retain existing provider IDs and credentials.
- OpenAI-compatible endpoints share a preset editor; NVIDIA remains a manufacturer entry with its existing provider and credential ID. Vendor SVGs precede service and model names; never use letter placeholders. Show API Key and model first; group endpoint, region, API plan and credential persistence under “接口设置”. The model field always accepts manual input, with a Bits Popover/Command searchable list, inline loading/error/retry and stale-response protection when endpoint/service/credentials change. Connection tests use a reserved inline feedback slot with accessible detail tooltip, never a toast or a growing warning panel; invalidate the result on draft changes. Keep protocol-specific providers distinct. Free interfaces stay visibly identified as unofficial. Custom OCR retains vision and HTTP JSON protocols, optional vault credentials, upload disclosure and a collapsed protocol example.

- Center the entire home shortcut cluster vertically with a modest upward optical offset (16px at comfortable heights). Keep top/bottom whitespace balanced; use auto margins and safe scrolling rather than positioning at the top or translating overflowing content.

- On Windows, resizing may temporarily use tiny native outline surfaces owned by the image window, with the same blue stroke and white handles. They do not host WebViews or accept input. Keep the final image geometry and viewport synchronized before restoring the CSS outline; release native surfaces on gesture end, hide, replacement or owner destruction. The 8px padding, 48px reservation, narrow-image allowance, and ready/presented protocol remain unchanged.

## Reference adaptation: ChatClaw

Reference ChatClaw's concise manufacturer directory, neutral surfaces and restrained control feedback. Keep the existing Svelte/Bits UI implementation. Provider SVG gradients must use instance-scoped IDs, including hidden selector copies. No page-position animation on module changes. Interface text uses local Chinese/English messages; user content and translated text are never localized as UI.

## Reference adaptation: FlClash

Use the user-selected centered About layout: application icon, name, version pill and one update action, followed by one rounded list containing automatic update checks and the three resource links. Do not add slogans, long repository URLs, extra licence badges or a second page title. Keep a stable status slot for update feedback. About scrolls naturally inside the settings panel at minimum height and 150% text.

Settings has three tabs: General, Appearance and About. Keep common language/translation/window controls visible. Put startup, automatic clipboard copying and configuration management in a labeled More settings disclosure; show legacy import only when a legacy configuration exists. Appearance keeps theme previews and font size visible, with accent choices in a disclosure. Preserve every preference, automatic saving, failure recovery and current credentials.

Apply the same tonal grouping, pill selection, rounded fields, circular action wells and restrained pressed/hover feedback to shortcuts, translation, services and dialogs. Screenshot selection geometry and the eight white anchors remain unchanged. Do not copy FlClash code or introduce Flutter or a second component framework.

## Updates and clipboard feedback

Successful copy feedback stays on the initiating button as a Check, with independent timers for source and translation. Never show a success toast or screenshot notice for copying; retain recoverable failure feedback. First launch opens the compact guide; Get started saves completion and navigates to Shortcuts. About contains a default-on automatic-check preference, shared download progress and an explicit restart confirmation. Never install unsigned bytes or silently restart ongoing work. Keep update signing secrets outside public source and cleanup targets.

About retains keyboard/wheel scrolling at small sizes but hides the decorative scrollbar. On Windows, update actions occupy a tiny owned native caption control left of system buttons, using the shared theme; never add another WebView or change the main client area. The tray version is dim when current, bright and actionable when newer. Tray actions may download and install directly, with the same busy/dirty checks; caption and About retain restart confirmation.
