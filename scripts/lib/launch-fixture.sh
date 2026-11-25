# The fixture owns both the v3 catalog and the expectations for its output.
prepare_launch_fixture() {
  local suffix
  suffix="$(basename "$PROBE_DIR" | tr -cd '[:alnum:]')"
  SOURCE_ID="smoke-source"
  SOURCE_MOUNT="probe-notes"
  SOURCE_ROOT="$PROBE_DIR/source"
  INITIAL_SLUG="initial"
  INITIAL_TERM="ccresdocinitial$suffix"
  FRESH_SLUG="fresh"
  FRESH_TERM="ccresdocfresh$suffix"
  CONFIG_PATH="$PROBE_DIR/config.toml"
  mkdir -p "$SOURCE_ROOT"
  write_launch_skill "$INITIAL_SLUG" "$INITIAL_TERM"
  local quoted_root
  quoted_root="$(node -p 'JSON.stringify(process.argv[1])' "$SOURCE_ROOT")" || return 1
  cat > "$CONFIG_PATH" <<TOML
schema_version = 3

[server]
preferred_port = 4892
fallback_to_free_port = true

[[document_sources]]
id = "$SOURCE_ID"
label = "Synthetic Probe Notes"
enabled = true
root = $quoted_root
mount = "$SOURCE_MOUNT"

[document_sources.scan_root]
kind = "same"

[document_sources.rules]
kind = "claude_code"
definition_version = 1
TOML
}

write_launch_skill() {
  local slug="$1" term="$2"
  mkdir -p "$SOURCE_ROOT/skills/$slug"
  printf -- '---\nname: "%s"\ndescription: Synthetic launch smoke fixture.\n---\n\nWatcher and search marker %s.\n' \
    "$term" "$term" > "$SOURCE_ROOT/skills/$slug/SKILL.md"
}

launch_fixture_route() {
  printf '/docs/%s/skills/%s/\n' "$SOURCE_MOUNT" "$1"
}

launch_fixture_in_index() {
  local index="$1" slug="$2" term="$3"
  jq -e --arg id "$SOURCE_ID:$SOURCE_MOUNT/skills/$slug" \
    --arg url "$(launch_fixture_route "$slug")" --arg term "$term" \
    'any(.[]; .id == $id and .url == $url and .title == $term and (.body | contains($term)))' \
    "$index" >/dev/null
}
