# Appearance transition table

`appearance-transitions.json` is the single source of truth for the behaviour
of the `update_appearance` Tauri command
(`src-tauri/src/settings_commands.rs`, `update_appearance`). Two consumers check
it:

- the Rust parity test, against the real resolver;
- the browser-navigation harness stub (`installTauriHarness` in
  `scripts/confirm-browser-navigation.mjs`).

Every expectation was derived from the backend code
(`settings_commands.rs`, `appearance.rs` `AppearanceState::envelope`,
`settings.rs` `SettingsStore::update_appearance`). When this table and the code
disagree, the code wins.

## Do not change expectations silently

A consumer that finds a row contradicting the backend must fix the row in its
own PR and call the change out in the PR description. A consumer must never
loosen its assertion or skip a row to make it pass.

## File layout

```json
{
  "schemaVersion": 1,
  "description": "...",
  "constants": {
    "effectivePort": 4892,
    "callerPath": "/docs/",
    "defaults": { "mode": "system", "themePack": "default" },
    "revisionPlaceholders": { "stored": "rev:stored", "afterSave": "rev:after-save" }
  },
  "rows": [ ... ]
}
```

- `effectivePort` is the active docs port for every row.
- The caller's URL is `input.callerOrigin + constants.callerPath`. The command
  accepts only `http` on `localhost` or `127.0.0.1`, at `effectivePort`, under
  `/docs/`.
- `defaults` is the appearance projected from a missing config
  (`SettingsDraft::defaults`).

## Row schema

Each row has the keys `{ name, parity, input, request, expect }`.

- `name`: what the row proves.
- `parity`: either `"rust+stub"` (both consumers enforce the row) or
  `"rust-only"` (only the Rust parity test enforces it).
- `input`: the state before the call.
  - `configStatus`: `missing` | `valid` | `malformed` | `invalid` |
    `unsupported_version` | `unreadable`. These are the `LoadStatus` wire
    values.
  - `stored`: `{mode, themePack}` when `valid`; otherwise `null`.
  - `preview`: `null` | `{mode, themePack}`. This is the active Settings
    preview.
  - `candidate`: `null` | `{origin, appearance: {mode, themePack}}`. This is
    the in-memory legacy candidate.
  - `callerOrigin`: the origin of the main window's current URL.
  - `availableThemePacks`: the theme packs that the settings store accepts.
  - `revision`: the stored revision. It is `null` for `missing` and
    `unreadable`; the real backend does not compute a revision for either.
    Otherwise it is the placeholder `"rev:stored"`.
- `request`: the exact wire object passed as `args.request` to
  `invoke("update_appearance", { request })`. The keys are camelCase and the
  enum values are snake_case: `{mode, themePack, intent, field}`. Some rows
  deliberately send an invalid shape.
- `expect`: one of two shapes.
  - Success:
    `{ envelope: {appearance, authoritative, revision, source, authoritativeSource}, emitsEvent, candidateAfter, storedAfter }`.
    - `envelope` is the `AppearanceEnvelope` that the command returns.
      `source` and `authoritativeSource` are `authoritative` | `preview` |
      `legacy_candidate` | `default`.
    - `envelope.revision` is `null` (the config is still missing),
      `"rev:stored"` (the input revision, unchanged), or `"rev:after-save"`.
      The last placeholder means a non-null revision that differs from the
      input revision, because the content hash of the rewritten file changed.
    - `emitsEvent`: whether `ccresdoc://appearance` is emitted. When it is
      emitted, the payload equals the returned envelope.
    - `candidateAfter`: the in-memory candidate after the call (`null` when it
      is cleared or absent).
    - `storedAfter`: the stored `{mode, themePack}` after the call. `null`
      means that the config is still missing. Any non-null value means that the
      config is now `valid`.
  - Error: `{ error: "<code>" }`. The code is `CommandError.code` from the
    rejected invoke. On every error row, nothing changes: the stored value, the
    revision, the preview and the candidate stay the same, and no event is
    emitted.

`update_appearance` never changes the preview. Only `preview_appearance` and
`clear_appearance_preview` change it.

## Error codes

| Code | Source |
| --- | --- |
| `forbidden_origin` | `authorize_docs_url`, which is checked before the theme pack |
| `invalid_theme_pack` | `validate_appearance`, which validates `themePack` for **both** intents and both `field` values, before any status check |
| `malformed` | `SaveError::Malformed` |
| `latest_not_valid` | `SaveError::LatestNotValid`, which is the error for an `invalid` config |
| `unsupported_version` | `SaveError::UnsupportedVersion` |
| `unreadable` | `SaveError::Unreadable` |
| `invalid_args` | Fixture sentinel (see below) |

`invalid_args` has no matching `CommandError`. Tauri deserializes
`AppearanceRequest` before the command body runs. On failure, the invoke
rejects with a plain string, not a `{code}` object:
``invalid args `request` for command `update_appearance`: ...``. The consumers
assert the following:

- The Rust test asserts that
  `serde_json::from_value::<AppearanceRequest>(request)` fails.
- The stub must reject the invoke before it touches any state.

Deserialization fails for the following reasons, because the request uses
`deny_unknown_fields` and has no optional keys:

- an unknown extra key;
- an unknown `intent`, `field` or `mode`;
- a missing key.

## Parity scope

The JS stub models only the `missing` and `valid` config statuses. It does not
emulate the settings store, so it cannot represent the `malformed`, `invalid`,
`unsupported_version` and `unreadable` statuses. The rows for those statuses are
`"rust-only"`, and only the Rust parity test enforces them. The stub must still
satisfy every `"rust+stub"` row.

The following are out of scope for this table:

- The `forbidden_window` rejection, which applies to a caller that is not the
  `main` window. The stub has no window labels, and an existing unit test in
  `settings_commands.rs` covers this rejection.
- Revision conflicts and I/O failures during the save.
- `legacy_candidate` with a non-missing, non-valid status. That case returns
  `envelope(latest)` with the status's default projection.

## Contract notes (code vs. the epic #302 summary)

- An `invalid` config makes `persist` fail with the code `latest_not_valid`. No
  error code is named `invalid`.
- `validate_appearance` checks `themePack` even when `field` is `mode` and even
  for `legacy_candidate`. A request whose untouched `themePack` is unknown is
  therefore rejected.
- A deserialization failure returns a plain Tauri string error, not a
  `CommandError` code. See `invalid_args` above.
- `legacy_candidate` with a present config does not report or clear a
  candidate. Any earlier candidate is kept, and the envelope ignores it.
- `legacy_candidate` with a missing config replaces any earlier candidate,
  including its origin.
- `envelope()` uses the candidate for any origin. `candidate_for(origin)` is not
  used on this path.
