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
        -- first group is the header; its fourth child owns ThemeToggle. This
        -- bounded path avoids traversing hundreds of generated sidebar nodes.
        set candidate to UI element 1 of UI element 4 of UI element 1 of UI element 1 of UI element 1 of UI element 1 of UI element 1 of front window
        set label to (description of candidate as text)
        if (role of candidate as text) is "AXButton" and label contains "Switch to " then
          set targetButton to candidate
        end if
      end try
      if targetButton is not missing value then exit repeat
      delay 0.1
    end repeat
    if targetButton is missing value then error "hydrated ThemeToggle button was not exposed by WebKit"

    set beforeLabel to ""
    try
      set beforeLabel to (description of targetButton as text)
    end try
    if beforeLabel is "" then
      try
        set beforeLabel to (title of targetButton as text)
      end try
    end if
    if beforeLabel is "" then
      try
        set beforeLabel to (name of targetButton as text)
      end try
    end if
    click targetButton
    delay 0.35

    set afterLabel to ""
    repeat with attempt from 1 to 50
      try
        set candidate to UI element 1 of UI element 4 of UI element 1 of UI element 1 of UI element 1 of UI element 1 of UI element 1 of front window
        set label to (description of candidate as text)
        if (role of candidate as text) is "AXButton" and label contains "Switch to " then
          set afterLabel to label
        end if
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
        set candidate to UI element 1 of UI element 4 of UI element 1 of UI element 1 of UI element 1 of UI element 1 of UI element 1 of front window
        set label to (description of candidate as text)
        if (role of candidate as text) is "AXButton" and label contains "Switch to " then
          set restoredLabel to label
        end if
      end try
      if restoredLabel is beforeLabel then exit repeat
      delay 0.1
    end repeat
    if restoredLabel is not beforeLabel then error "ThemeToggle did not restore its initial mode label"
    return "ThemeToggle interactive: " & beforeLabel & " -> " & afterLabel & " -> " & restoredLabel & " (restored)"
  end tell
end tell
