-- Native macOS menu and shortcut smoke used by scripts/test-launch.sh --controls.
-- It inspects the application menu before sending the real ⌘H shortcut,
-- then proves the same process can be reactivated with Settings still hidden.

tell application "System Events"
  if not (exists process "CCResDoc") then error "CCResDoc is not running"
  tell process "CCResDoc"
    set frontmost to true
    if (count of windows) = 0 then error "CCResDoc has no window"
    set pidBeforeHide to unix id

    -- Wait for the native menu hierarchy to be published before inspecting it.
    set appMenu to missing value
    repeat with attempt from 1 to 100
      try
        -- The Apple menu occupies item 1; the application's menu is the
        -- named CCResDoc item that follows it.
        set candidateMenu to menu 1 of menu bar item "CCResDoc" of menu bar 1
        if (count of menu items of candidateMenu) > 0 then
          set appMenu to candidateMenu
          exit repeat
        end if
      end try
      delay 0.1
    end repeat
    if appMenu is missing value then error "CCResDoc application menu was not exposed by Accessibility"

    set appMenuItems to menu items of appMenu
    set aboutIndex to 0
    set settingsIndex to 0
    set quitIndex to 0
    set hideItem to missing value
    set hideOthersItem to missing value
    set showAllItem to missing value
    set servicesItem to missing value
    repeat with itemIndex from 1 to (count of appMenuItems)
      set candidateItem to item itemIndex of appMenuItems
      set candidateName to ""
      try
        set candidateName to (name of candidateItem as text)
      end try
      if candidateName is "About CCResDoc" then set aboutIndex to itemIndex
      if candidateName is "Settings…" then set settingsIndex to itemIndex
      if candidateName is "Quit CCResDoc" then set quitIndex to itemIndex
      if candidateName is "Hide CCResDoc" then set hideItem to candidateItem
      if candidateName is "Hide Others" then set hideOthersItem to candidateItem
      if candidateName is "Show All" then set showAllItem to candidateItem
      if candidateName is "Services" then set servicesItem to candidateItem
    end repeat

    if hideItem is missing value then error "CCResDoc app menu is missing Hide CCResDoc"
    if hideOthersItem is missing value then error "CCResDoc app menu is missing Hide Others"
    if showAllItem is missing value then error "CCResDoc app menu is missing Show All"
    if servicesItem is missing value then error "CCResDoc app menu is missing Services"
    if aboutIndex = 0 then error "CCResDoc app menu is missing About CCResDoc"
    if settingsIndex = 0 then error "CCResDoc app menu is missing Settings…"
    if quitIndex = 0 then error "CCResDoc app menu is missing Quit CCResDoc"
    if not (aboutIndex < settingsIndex and settingsIndex < quitIndex) then error "CCResDoc app menu order is not About, Settings…, Quit"

    set hideCommandChar to ""
    try
      set hideCommandChar to (value of attribute "AXMenuItemCmdChar" of hideItem as text)
    on error
      error "Hide CCResDoc has no AXMenuItemCmdChar"
    end try
    ignoring case
      if hideCommandChar is not "h" then error "Hide CCResDoc AXMenuItemCmdChar is not H"
    end ignoring
    set hideCommandModifiers to 0
    try
      set hideCommandModifiers to (value of attribute "AXMenuItemCmdModifiers" of hideItem as integer)
    on error
      error "Hide CCResDoc has no AXMenuItemCmdModifiers"
    end try
    if hideCommandModifiers is not 0 then error "Hide CCResDoc AXMenuItemCmdModifiers is not 0"

    -- Verify the standard Window and View menu contracts while the app is up.
    set windowMenu to missing value
    set viewMenu to missing value
    repeat with attempt from 1 to 100
      try
        if (exists menu bar item "Window" of menu bar 1) and (exists menu bar item "View" of menu bar 1) then
          set windowMenu to menu 1 of menu bar item "Window" of menu bar 1
          set viewMenu to menu 1 of menu bar item "View" of menu bar 1
          if (count of menu items of windowMenu) > 0 and (count of menu items of viewMenu) > 0 then exit repeat
        end if
      end try
      delay 0.1
    end repeat
    if windowMenu is missing value then error "CCResDoc Window and View menus were not exposed"

    set minimizeItem to missing value
    set zoomItem to missing value
    set windowItems to menu items of windowMenu
    repeat with itemIndex from 1 to (count of windowItems)
      set candidateItem to item itemIndex of windowItems
      set candidateName to ""
      try
        set candidateName to (name of candidateItem as text)
      end try
      if candidateName is "Minimize" then set minimizeItem to candidateItem
      if candidateName is "Zoom" then set zoomItem to candidateItem
    end repeat
    if minimizeItem is missing value then error "Window menu is missing Minimize"
    if zoomItem is missing value then error "Window menu is missing Zoom"
    set minimizeCommandChar to ""
    try
      set minimizeCommandChar to (value of attribute "AXMenuItemCmdChar" of minimizeItem as text)
    on error
      error "Window Minimize has no AXMenuItemCmdChar"
    end try
    ignoring case
      if minimizeCommandChar is not "m" then error "Window Minimize AXMenuItemCmdChar is not M"
    end ignoring
    set minimizeCommandModifiers to 0
    try
      set minimizeCommandModifiers to (value of attribute "AXMenuItemCmdModifiers" of minimizeItem as integer)
    on error
      error "Window Minimize has no AXMenuItemCmdModifiers"
    end try
    if minimizeCommandModifiers is not 0 then error "Window Minimize AXMenuItemCmdModifiers is not 0"

    set viewItems to menu items of viewMenu
    if (count of viewItems) = 0 then error "View menu has no items"
    set lastViewItem to item (count of viewItems) of viewItems
    set lastViewName to ""
    try
      set lastViewName to (name of lastViewItem as text)
    end try
    if lastViewName is not "Toggle Full Screen" then error "View menu does not end with Toggle Full Screen"

    -- Create the Settings window, then close it through the native Escape path.
    -- The app's lifecycle handler hides this window instead of destroying it.
    keystroke "," using command down
    set settingsWindow to missing value
    repeat with attempt from 1 to 100
      try
        if exists window "CCResDoc Settings" then
          set settingsWindow to window "CCResDoc Settings"
          if (visible of settingsWindow) is true then exit repeat
        end if
      end try
      delay 0.1
    end repeat
    if settingsWindow is missing value then error "Command-comma did not open CCResDoc Settings"
    if (visible of settingsWindow) is not true then error "CCResDoc Settings did not become visible"
    key code 53
    set settingsHiddenBeforeHide to false
    repeat with attempt from 1 to 100
      try
        if (visible of settingsWindow) is false then
          set settingsHiddenBeforeHide to true
          exit repeat
        end if
      end try
      delay 0.1
    end repeat
    if not settingsHiddenBeforeHide then error "Escape did not hide CCResDoc Settings"

    -- Send the actual shortcut. Clicking Hide CCResDoc would not prove that
    -- AppKit dispatched ⌘H to this menu item.
    set frontmost to true
    keystroke "h" using command down
    set appHidden to false
    repeat with attempt from 1 to 100
      try
        if (visible of process "CCResDoc") is false then
          set appHidden to true
          exit repeat
        end if
      end try
      delay 0.1
    end repeat
    if not appHidden then error "Command-H did not hide CCResDoc"

    tell application id "com.takazudo.ccresdoc" to activate
    set appVisibleAgain to false
    repeat with attempt from 1 to 100
      try
        if (visible of process "CCResDoc") is true then
          set appVisibleAgain to true
          exit repeat
        end if
      end try
      delay 0.1
    end repeat
    if not appVisibleAgain then error "CCResDoc did not become visible after reactivation"
    if (unix id of process "CCResDoc") is not pidBeforeHide then error "CCResDoc process changed across hide/unhide"
    set windowsRestored to false
    repeat with attempt from 1 to 100
      try
        if (count of windows) > 0 then
          set windowsRestored to true
          exit repeat
        end if
      end try
      delay 0.1
    end repeat
    if not windowsRestored then error "CCResDoc has no window after reactivation"

    set settingsVisibleAgain to false
    try
      if exists window "CCResDoc Settings" then set settingsVisibleAgain to (visible of window "CCResDoc Settings")
    end try
    if settingsVisibleAgain then error "CCResDoc Settings became visible after reactivation"

    return "CCResDoc native menu/Command-H hide: app menu, Window, View, PID " & pidBeforeHide & " verified; app reactivated with Settings hidden"
  end tell
end tell
