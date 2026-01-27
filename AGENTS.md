# AGENTS.md

These instructions apply to all work in this repository.

## Architecture: Vertical Slice First

Organize code by feature (slice), not by technical layer.

### What is a slice?
A slice owns everything needed for a feature end‑to‑end:
- UI components and templates
- State/store logic
- Domain/types and feature utilities
- Data access (Tauri commands, services, adapters)
- Feature tests

### Where slices live
Keep slices under `src/app/features/` (Angular app) with a clear feature folder.

Example structure:
- `src/app/features/tasks/` (slice)
  - `components/` (feature‑specific UI)
  - `store/` or `task.store.ts`
  - `task.models.ts`
  - `task.utils.ts`
  - `task.routes.ts` (if needed later)
- `src/app/features/launcher/` (slice)
- `src/app/shared/` (shared UI/utilities only)

### Rules
1. **Default to a slice**: new functionality must be placed in the owning slice.
2. **Keep slices self‑contained**: avoid reaching into other slices’ internals.
3. **Prefer local utilities**: put helpers in the slice unless they are used by 2+ slices.
4. **Shared is small**: only truly shared UI or utils go in `src/app/shared/`.
5. **No “layer folders”**: avoid new global `components/`, `services/`, `models/` directories.
6. **Imports flow inward**: shared → slices; avoid slice‑to‑slice dependencies.
7. **Feature tests live with the slice**: keep tests alongside slice code.

### When editing existing code
- If a file already belongs to a slice, keep related changes in that slice.
- If you touch cross‑slice logic, consider pulling it into a shared utility only if used in multiple slices.
- Avoid moving files across slices unless the feature ownership is clearly wrong.

### Tauri side
For Rust (`src-tauri/`), prefer grouping by feature module as well:
- `src-tauri/src/features/tasks/` (including `src-tauri/src/features/tasks/agents/`), `src-tauri/src/features/launcher/`
- Keep commands, models, and helpers close to the feature module.

### If uncertain
Ask the user which slice should own the change before creating new structure.
