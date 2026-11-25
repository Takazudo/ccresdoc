run_evidenced_check() {
  local name="$1" status=0
  shift
  printf '%q ' "$@" > "$CCRESDOC_EVIDENCE_DIR/$name.command"
  printf '\n' >> "$CCRESDOC_EVIDENCE_DIR/$name.command"
  "$@" > "$CCRESDOC_EVIDENCE_DIR/$name.log" 2>&1 || status=$?
  printf '%s\n' "$status" > "$CCRESDOC_EVIDENCE_DIR/$name.exit"
  if ! node "$IDENTITY_SCRIPT" "$APP_PATH" "$REVISION" > "$CCRESDOC_EVIDENCE_DIR/$name.identity.json" ||
     ! cmp -s "$CCRESDOC_EVIDENCE_DIR/bundle.identity.json" "$CCRESDOC_EVIDENCE_DIR/$name.identity.json"; then
    echo "FAIL $name: retained bundle missing or identity changed" >&2
    return 1
  fi
  printf '%s exit=%s; bundle identity verified; log: %s/%s.log\n' "$name" "$status" "$CCRESDOC_EVIDENCE_DIR" "$name"
  return "$status"
}
