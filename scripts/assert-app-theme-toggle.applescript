-- Accessibility smoke used by scripts/test-launch.sh --controls.
-- It finds the hydrated header ThemeToggle by its AX label, clicks it once,
-- and requires the label to change.  That is an interaction assertion, not a
-- network or livereload wait: the caller invokes it only after /docs/ is ready.

tell application "System Events"
  if not (exists process "CCResDoc") then error "CCResDoc is not running"
  tell process "CCResDoc"
    set frontmost to true
    if (count of windows) = 0 then error "CCResDoc has no window"
    set targetButton to missing value
    -- The sidecar's /docs/ can become ready just before the host navigation
    -- has published the WebKit AX tree. Wait for that host-owned boundary,
    -- never for a livereload event.
    repeat with attempt from 1 to 150
      try
        -- Tauri wraps WKWebView in two groups and one scroll area. The page's
        -- first group is the header. Search only its direct children and
        -- direct grandchildren, whose small set remains stable as header
        -- slots are added; never traverse the generated sidebar tree.
        set targetButton to my find_theme_toggle(front window)
      end try
      if targetButton is not missing value then exit repeat
      delay 0.1
    end repeat
    if targetButton is missing value then error "hydrated ThemeToggle button was not exposed by WebKit"

    set beforeLabel to ""
    set beforeLabel to my theme_toggle_label(targetButton)
    click targetButton
    delay 0.35

    set afterLabel to ""
    repeat with attempt from 1 to 50
      try
        set candidate to my find_theme_toggle(front window)
        if candidate is not missing value then set afterLabel to my theme_toggle_label(candidate)
      end try
      if afterLabel is not "" and afterLabel is not beforeLabel then exit repeat
      delay 0.1
    end repeat
    if afterLabel is "" then error "ThemeToggle disappeared after activation"
    if beforeLabel is afterLabel then error "ThemeToggle did not change its mode label after activation"

    -- Put the control back in the state this smoke observed on entry so the
    -- interaction assertion does not leave the user's selected mode flipped.
    click candidate
    set restoredLabel to ""
    repeat with attempt from 1 to 50
      try
        set candidate to my find_theme_toggle(front window)
        if candidate is not missing value then set restoredLabel to my theme_toggle_label(candidate)
      end try
      if restoredLabel is beforeLabel then exit repeat
      delay 0.1
    end repeat
    if restoredLabel is not beforeLabel then error "ThemeToggle did not restore its initial mode label"
    return "ThemeToggle interactive: " & beforeLabel & " -> " & afterLabel & " -> " & restoredLabel & " (restored)"
  end tell
end tell

on find_theme_toggle(windowElement)
  tell application "System Events"
    try
      -- This is the existing bounded route to the header group. The search
      -- below intentionally stops after one child level.
      set headerGroup to UI element 1 of UI element 1 of UI element 1 of UI element 1 of UI element 1 of windowElement
      set headerChildren to UI elements of headerGroup
      repeat with child in headerChildren
        set candidate to contents of child
        if my is_theme_toggle(candidate) then return candidate
        try
          set grandchildren to UI elements of candidate
          repeat with grandchild in grandchildren
            set candidate to contents of grandchild
            if my is_theme_toggle(candidate) then return candidate
          end repeat
        end try
      end repeat
    on error
      return missing value
    end try
  end tell
  return missing value
end find_theme_toggle

on is_theme_toggle(candidate)
  tell application "System Events"
    try
      if (role of candidate as text) is not "AXButton" then return false
    on error
      return false
    end try
    repeat with attributeName in {"description", "title", "name"}
      set candidateLabel to my candidate_attribute(candidate, contents of attributeName)
      ignoring case
        if candidateLabel contains "Switch to " then return true
      end ignoring
    end repeat
  end tell
  return false
end is_theme_toggle

on theme_toggle_label(candidate)
  tell application "System Events"
    repeat with attributeName in {"description", "title", "name"}
      set candidateLabel to my candidate_attribute(candidate, contents of attributeName)
      ignoring case
        if candidateLabel contains "Switch to " then return candidateLabel
      end ignoring
    end repeat
  end tell
  return ""
end theme_toggle_label

on candidate_attribute(candidate, attributeName)
  tell application "System Events"
    try
      if attributeName is "description" then return (description of candidate as text)
      if attributeName is "title" then return (title of candidate as text)
      if attributeName is "name" then return (name of candidate as text)
    on error
      return ""
    end try
  end tell
  return ""
end candidate_attribute
