-- Native macOS search/find smoke used by scripts/test-launch.sh --controls.
--
-- This deliberately drives both surfaces through Accessibility.  The search
-- surface is exercised with a real Command-K keystroke, a real text entry,
-- and a result-link activation.  The find surface is exercised with a real
-- Command-F keystroke and checks only the controls that WebKit exposes in AX;
-- DOM-only match markup belongs to the browser harness in issue #199.

on run argv
  if (count of argv) < 1 then error "search smoke requires a non-empty indexed term"
  set searchTerm to item 1 of argv
  if searchTerm is "" then error "search smoke requires a non-empty indexed term"

  tell application "System Events"
    if not (exists process "CCResDoc") then error "CCResDoc is not running"
    tell process "CCResDoc"
      set frontmost to true
      if (count of windows) = 0 then error "CCResDoc has no window"

      -- ⌘K must be dispatched by AppKit/WebKit, rather than by clicking the
      -- header button.  The bounded wait also covers a cold WebView AX tree.
      keystroke "k" using command down
      set searchDialog to my wait_for_search_dialog(front window, 150)
      if searchDialog is missing value then error "Command-K did not expose the Search dialog in Accessibility"

      set searchInput to my wait_for_search_input(searchDialog, 150)
      if searchInput is missing value then error "Search dialog did not expose its text field in Accessibility"
      click searchInput
      keystroke "a" using command down
      keystroke searchTerm

      set resultLink to my wait_for_result_link(searchDialog, searchTerm, 200)
      if resultLink is missing value then error "Search dialog returned no accessible result for term '" & searchTerm & "'"
      set resultURL to my ui_url(resultLink)
      if resultURL is "" then error "Search result did not expose an accessible URL"
      set routeMarker to my docs_path_marker(resultURL)
      if routeMarker is "" then error "Search result URL is not a /docs/ route: " & resultURL

      click resultLink
      set navigationState to my wait_for_navigation(front window, routeMarker, 200)
      if navigationState is not "navigated" then error "Activating the search result did not navigate to " & routeMarker

      -- The result click must close the modal before the next native shortcut.
      -- wait_for_navigation includes the closed-dialog assertion.
      keystroke "f" using command down
      set findInput to my wait_for_find_input(front window, 150)
      if findInput is missing value then error "Command-F did not expose the Find in page field in Accessibility"
      set controlsState to my wait_for_find_controls(front window, 100)
      if controlsState is "" then error "Find in page controls were not all visible in Accessibility"

      -- Escape is the shipped find-bar close path.  Confirm that the visible
      -- input is gone so a later invocation cannot inherit a stale surface.
      key code 53
      set findClosed to false
      repeat with attempt from 1 to 100
        if (my find_input(front window)) is missing value then
          set findClosed to true
          exit repeat
        end if
        delay 0.1
      end repeat
      if not findClosed then error "Escape did not close the Find in page controls"

      return "Search Command-K term '" & searchTerm & "' activated " & routeMarker & "; Command-F controls visible (Prev, Next, Close)"
    end tell
  end tell
end run

on all_accessible_elements(rootElement)
  tell application "System Events"
    try
      return entire contents of rootElement
    on error
      return {}
    end try
  end tell
end all_accessible_elements

on ui_role(candidate)
  tell application "System Events"
    try
      return (role of candidate) as text
    on error
      try
        return (value of attribute "AXRole" of candidate) as text
      on error
        return ""
      end try
    end try
  end tell
end ui_role

on ui_label(candidate)
  tell application "System Events"
    set labelText to ""
    repeat with attributeName in {"AXDescription", "AXTitle", "AXName", "AXValue"}
      try
        set candidateText to (value of attribute (contents of attributeName) of candidate) as text
        if candidateText is not "" then
          set labelText to candidateText
          exit repeat
        end if
      end try
    end repeat
    if labelText is "" then
      try
        set labelText to (description of candidate) as text
      end try
    end if
    if labelText is "" then
      try
        set labelText to (name of candidate) as text
      end try
    end if
    return labelText
  end tell
end ui_label

on ui_visible(candidate)
  tell application "System Events"
    try
      return (value of attribute "AXVisible" of candidate) as boolean
    on error
      try
        return visible of candidate
      on error
        return true
      end try
    end try
  end tell
end ui_visible

on ui_url(candidate)
  tell application "System Events"
    repeat with attributeName in {"AXURL", "AXLink", "AXValue"}
      try
        set candidateURL to (value of attribute (contents of attributeName) of candidate) as text
        if candidateURL starts with "http://" or candidateURL starts with "https://" or candidateURL starts with "/docs/" then return candidateURL
      end try
    end repeat
  end tell
  return ""
end ui_url

on find_search_dialog(windowElement)
  set candidates to my all_accessible_elements(windowElement)
  repeat with candidate in candidates
    set roleName to my ui_role(contents of candidate)
    if roleName is "AXDialog" or roleName is "AXSheet" then
      set labelText to my ui_label(contents of candidate)
      ignoring case
        if labelText contains "search" then return contents of candidate
      end ignoring
    end if
  end repeat
  return missing value
end find_search_dialog

on wait_for_search_dialog(windowElement, attempts)
  repeat with attempt from 1 to attempts
    set candidate to my find_search_dialog(windowElement)
    if candidate is not missing value then return candidate
    delay 0.1
  end repeat
  return missing value
end wait_for_search_dialog

on find_search_input(rootElement)
  set candidates to my all_accessible_elements(rootElement)
  repeat with candidate in candidates
    set candidate to contents of candidate
    if (my ui_role(candidate)) is "AXTextField" then
      set labelText to my ui_label(candidate)
      ignoring case
        if labelText does not contain "find in page" and labelText does not contain "filter navigation" then
          if my ui_visible(candidate) then return candidate
        end if
      end ignoring
    end if
  end repeat
  return missing value
end find_search_input

on wait_for_search_input(dialogElement, attempts)
  repeat with attempt from 1 to attempts
    set candidate to my find_search_input(dialogElement)
    if candidate is not missing value then return candidate
    delay 0.1
  end repeat
  return missing value
end wait_for_search_input

on find_result_link(dialogElement, term)
  set candidates to my all_accessible_elements(dialogElement)
  repeat with candidate in candidates
    set candidate to contents of candidate
    if (my ui_role(candidate)) is "AXLink" and my ui_visible(candidate) then
      set candidateURL to my ui_url(candidate)
      if candidateURL contains "/docs/" then
        -- Search results are the only /docs/ links in the modal. Prefer a
        -- label containing the requested term, but accept the first routed
        -- result when WebKit exposes only its URL in AX.
        set labelText to my ui_label(candidate)
        ignoring case
          if labelText contains term then return candidate
        end ignoring
        set fallbackLink to candidate
      end if
    end if
  end repeat
  try
    return fallbackLink
  on error
    return missing value
  end try
end find_result_link

on wait_for_result_link(dialogElement, term, attempts)
  repeat with attempt from 1 to attempts
    set candidate to my find_result_link(dialogElement, term)
    if candidate is not missing value then return candidate
    delay 0.1
  end repeat
  return missing value
end wait_for_result_link

on docs_path_marker(candidateURL)
  ignoring case
    if candidateURL does not contain "/docs/" then return ""
  end ignoring
  set oldDelimiters to AppleScript's text item delimiters
  set AppleScript's text item delimiters to "/docs/"
  set parts to text items of candidateURL
  set AppleScript's text item delimiters to oldDelimiters
  if (count of parts) < 2 then return ""
  return "/docs/" & item 2 of parts
end docs_path_marker

on current_web_area_url(windowElement)
  set candidates to my all_accessible_elements(windowElement)
  repeat with candidate in candidates
    set candidate to contents of candidate
    if (my ui_role(candidate)) is "AXWebArea" then
      set candidateURL to my ui_url(candidate)
      if candidateURL is not "" then return candidateURL
    end if
  end repeat
  return ""
end current_web_area_url

on search_surface_present(windowElement)
  set dialogElement to my find_search_dialog(windowElement)
  if dialogElement is not missing value then
    -- A closed HTML dialog can remain in WebKit's AX tree.  Its visible
    -- search text field is the stronger open-state boundary.
    if (my find_search_input(dialogElement)) is not missing value then return true
  end if
  set candidates to my all_accessible_elements(windowElement)
  repeat with candidate in candidates
    set candidate to contents of candidate
    if (my ui_role(candidate)) is "AXTextField" then
      set labelText to my ui_label(candidate)
      ignoring case
        if labelText does not contain "find in page" and labelText does not contain "filter navigation" and my ui_visible(candidate) then return true
      end ignoring
    end if
  end repeat
  return false
end search_surface_present

on wait_for_navigation(windowElement, routeMarker, attempts)
  set routeSeen to false
  set dialogClosed to false
  repeat with attempt from 1 to attempts
    set currentURL to my current_web_area_url(windowElement)
    ignoring case
      if currentURL contains routeMarker then set routeSeen to true
    end ignoring
    if not (my search_surface_present(windowElement)) then set dialogClosed to true
    if routeSeen and dialogClosed then return "navigated"
    delay 0.1
  end repeat
  if not routeSeen then return "route-missing"
  return "dialog-open"
end wait_for_navigation

on find_in_page_input(rootElement)
  set candidates to my all_accessible_elements(rootElement)
  repeat with candidate in candidates
    set candidate to contents of candidate
    if (my ui_role(candidate)) is "AXTextField" then
      set labelText to my ui_label(candidate)
      ignoring case
        if labelText contains "find in page" and my ui_visible(candidate) then return candidate
      end ignoring
    end if
  end repeat
  return missing value
end find_in_page_input

on find_input(rootElement)
  return my find_in_page_input(rootElement)
end find_input

on wait_for_find_input(windowElement, attempts)
  repeat with attempt from 1 to attempts
    set candidate to my find_in_page_input(windowElement)
    if candidate is not missing value then return candidate
    delay 0.1
  end repeat
  return missing value
end wait_for_find_input

on wait_for_find_controls(windowElement, attempts)
  repeat with attempt from 1 to attempts
    set previousVisible to false
    set nextVisible to false
    set closeVisible to false
    set candidates to my all_accessible_elements(windowElement)
    repeat with candidate in candidates
      set candidate to contents of candidate
      if (my ui_role(candidate)) is "AXButton" and my ui_visible(candidate) then
        set labelText to my ui_label(candidate)
        ignoring case
          if labelText is "prev" then set previousVisible to true
          if labelText is "next" then set nextVisible to true
          if labelText is "close" then set closeVisible to true
        end ignoring
      end if
    end repeat
    if previousVisible and nextVisible and closeVisible then return "ready"
    delay 0.1
  end repeat
  return ""
end wait_for_find_controls
