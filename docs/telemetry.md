# Telemetry and Diagnostics

GitSpace ships with optional, anonymized diagnostics. The experience is opt-in: users are prompted on first launch and can toggle the feature any time from **Settings → Privacy**.

## What is collected
- Launch and session metadata (counts, release channel).
- Feature usage signals such as tab switches, tab reordering, or repository openings.
- UI navigation context is captured with the trigger type (click, keyboard, context menu, drag-and-drop) to validate accessibility paths.
- Repository references are hashed before they leave the device; no file contents, commit messages, or credentials are recorded.

## How events are handled
- Events are batched (10 at a time) and sent to `https://telemetry.gitspace.local/events`.
- When offline or if delivery fails, events are cached locally in `~/.config/gitspace/telemetry-queue.json` and retried later.
- Disabling telemetry stops new collection and clears any pending, cached payloads.

## Controls and data purge
- The Privacy section in Settings contains a single checkbox to enable/disable collection plus a **Purge collected diagnostics** button.
- The purge button deletes pending batches and any offline cache in one step.
- Users can also decline the initial prompt; the app will honor that choice and not prompt again.
- Telemetry events respect opt-in state at emission time; UI navigation events are only sent after consent and include minimal properties (tab label, trigger, indices for reorders).
