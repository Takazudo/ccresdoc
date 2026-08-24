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
    repeat with candidate in (entire contents of front window)
      try
        if (role of candidate as text) is "AXButton" then
          set label to ""
          try
            set label to (description of candidate as text)
          end try
          if label is "" then
            try
              set label to (title of candidate as text)
            end try
          end if
          if label is "" then
            try
              set label to (name of candidate as text)
            end try
          end if
          if label contains "Switch to " then
            set targetButton to candidate
            exit repeat
          end if
        end if
      end try
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
    repeat with candidate in (entire contents of front window)
      try
        if (role of candidate as text) is "AXButton" then
          set label to ""
          try
            set label to (description of candidate as text)
          end try
          if label is "" then
            try
              set label to (title of candidate as text)
            end try
          end if
          if label is "" then
            try
              set label to (name of candidate as text)
            end try
          end if
          if label contains "Switch to " then
            set afterLabel to label
            exit repeat
          end if
        end if
      end try
    end repeat
    if afterLabel is "" then error "ThemeToggle disappeared after activation"
    if beforeLabel is afterLabel then error "ThemeToggle did not change its mode label after activation"
    return "ThemeToggle interactive: " & beforeLabel & " -> " & afterLabel
  end tell
end tell
