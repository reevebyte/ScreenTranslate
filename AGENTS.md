# ScreenTranslate contributor instructions

Before changing UI, read and follow [DESIGN.md](DESIGN.md). It is the source of truth for visual and interaction decisions.

- Preserve TypeScript, Svelte 5, Tailwind 4, shadcn-svelte, Bits UI, Phosphor, Vite and Tauri 2. Assess necessity before adding dependencies; prefer existing primitives and Svelte motion/transition.
- Change one window or related component group at a time; check and inspect it before proceeding. Use the semantic tokens, not new override layers, arbitrary per-page sizes or duplicated controls.
- Do not change Rust OCR, translation, capture, hotkeys, screen mapping, window lifecycle or IPC contracts as part of a visual refactor. Keep ready/presented signals and result layout constants intact.
- Preserve user edits. Do not reset the checkout, rewrite credentials or touch production configuration during visual QA.
- Verify light/dark themes, keyboard focus, reduced motion, 150% fonts, long text, errors and native minimum-size content layouts. Save before/after screenshots; distinguish browser mock previews from real native verification.
- Run existing `pnpm check`, `pnpm test`, `pnpm build` and `node scripts/measure-v2.mjs` as appropriate. Report failures and test limitations honestly.
- After every successful release build, keep the current executable and installers and remove earlier versioned build outputs, as explicitly requested by the user. Use scripts/tauri-v2.mjs; never clean on failure, remove newer/unversioned artifacts, or delete source, user configuration or the active build cache. Cleanup paths must resolve inside this workspace.
