use crate::AccessibilityUiElement;
use crate::FLOATING_APPLICATIONS;
use crate::Notification;
use crate::NotificationEvent;
use crate::REGEX_IDENTIFIERS;
use crate::TABBED_APPLICATIONS;
use crate::UNMANAGED_WINDOW_IDS;
use crate::WORKSPACE_MATCHING_RULES;
use crate::accessibility::AccessibilityApi;
use crate::accessibility::enhanced_ui;
use crate::accessibility::error::AccessibilityApiError;
use crate::accessibility::error::AccessibilityError;
use crate::accessibility::notification_constants::AccessibilityNotification;
use crate::animation::animator;
use crate::app_kit_notification_constants::AppKitWorkspaceNotification;
use crate::border_manager;
use crate::core::DefaultLayout;
use crate::core::Layout;
use crate::core::Rect;
use crate::core::WindowContainerBehaviour;
use crate::core::WindowHidingPosition;
use crate::core::config_generation::MatchingRule;
use crate::current_space_id;
use crate::macos_api::MacosApi;
use crate::notify_subscribers;
use crate::splash;
use crate::splash::mdm_enrollment;
use crate::state::State;
use crate::window::AdhocWindow;
use crate::window::RuleDebug;
use crate::window::Snapping;
use crate::window::Window;
use crate::window::should_act;
use crate::window_manager::WindowManager;
use crate::window_manager_event::ManualNotification;
use crate::window_manager_event::SystemNotification;
use crate::window_manager_event::WindowManagerEvent;
use crate::window_manager_event_listener;
use crate::workspace::WorkspaceLayer;
use crate::workspace_reconciliator;
use color_eyre::eyre;
use color_eyre::eyre::OptionExt;
use parking_lot::Mutex;
use std::process::Command;
use std::sync::Arc;
use tracing::instrument;

#[tracing::instrument]
pub fn listen_for_events(wm: Arc<Mutex<WindowManager>>) {
    let receiver = wm.lock().incoming_events.clone();

    std::thread::spawn(|| {
        loop {
            if let Ok((mdm, server)) = mdm_enrollment() {
                #[allow(clippy::collapsible_if)]
                if mdm && splash::should().map(|f| f.into()).unwrap_or(true) {
                    let mut args = vec!["splash".to_string()];
                    if let Some(server) = server {
                        args.push(server);
                    }

                    let _ = Command::new("komorebic").args(&args).spawn();
                }
            }

            std::thread::sleep(std::time::Duration::from_secs(14400));
        }
    });

    std::thread::spawn(move || {
        tracing::info!("listening");
        loop {
            if let Ok(event) = receiver.recv() {
                let mut guard = wm.lock();
                match guard.process_event(event) {
                    Ok(()) => {}
                    Err(error) => {
                        if cfg!(debug_assertions) {
                            tracing::error!("{:?}", error)
                        } else {
                            tracing::error!("{}", error)
                        }
                    }
                }
            }
        }
    });
}

impl WindowManager {
    #[instrument(skip_all)]
    pub fn process_event(&mut self, event: WindowManagerEvent) -> eyre::Result<()> {
        // the step is off the channel whatever is done about it, and the next that comes waits for
        // this one to begin and no longer, which is not so if it is left to where it is handled:
        // nothing is handled while paused, or on another space
        if matches!(event, WindowManagerEvent::ResizeStep(_, _, _)) {
            window_manager_event_listener::resize_step_taken();
        }

        if matches!(event, WindowManagerEvent::ScreenLock(_, _)) {
            let application = self.application(event.process_id())?;
            if application.name().unwrap_or_default() == "loginwindow" {
                tracing::debug!("pausing while screen is locked");
                self.is_paused = true;
            }
        }

        if matches!(event, WindowManagerEvent::ScreenUnlock(_, _)) {
            let application = self.application(event.process_id())?;
            if application.name().unwrap_or_default() == "loginwindow" {
                tracing::debug!("unpausing on screen unlock");
                self.is_paused = false;
            }
        }

        if self.is_paused {
            tracing::trace!("ignoring while paused");
            return Ok(());
        }

        if matches!(event, WindowManagerEvent::SpaceChange(_, _))
            && let Some(space_id) = &self.space_id
            && let Some(current_space_id) = current_space_id()
        {
            if *space_id == current_space_id {
                border_manager::send_notification(None, None, false);
            } else {
                border_manager::destroy_all_borders()?;
            }

            return Ok(());
        }

        if let Some(space_id) = &self.space_id
            && let Some(current_space_id) = current_space_id()
            && *space_id != current_space_id
        {
            tracing::trace!("ignoring events and commands while not on space {space_id}");
            border_manager::destroy_all_borders()?;
            return Ok(());
        }

        // A step of a drag says only that the window has moved, and is handled before anything that
        // is done for the other events: they ask the app of the window things, which is time that
        // the windows next to the edge spend not following it, and a step is not in anybody's
        // subscription for the same reason
        if let WindowManagerEvent::ResizeStep(_, _, window_id) = event {
            return self.live_resize_step(window_id);
        }

        let mut rule_debug = RuleDebug::default();

        let mut should_manage = true;
        {
            let application = self.application(event.process_id())?;
            if let Some(window_element) = application.main_window()
                && let Ok(window) = Window::new(window_element, application.clone())
            {
                let window_id = window.id;
                let print_window = window.clone();
                should_manage = window.should_manage(Some(event), &mut rule_debug)?;

                if UNMANAGED_WINDOW_IDS.lock().contains(&window_id) {
                    should_manage = false;
                }

                if !should_manage {
                    tracing::debug!(
                        "ignoring event as window should not be managed: {print_window}"
                    );
                }
            }
        }

        if !should_manage {
            return Ok(());
        }

        let mut window_id = None;
        let mut window_element = None;

        {
            let application = self.application(event.process_id())?;
            if let Some(element) = application.main_window()
                && let Ok(wid) = AccessibilityApi::window_id(&element)
            {
                window_id = Some(wid);
                window_element = Some(AccessibilityUiElement(element.clone()));
            }
        }

        // don't want to spam logs for manually triggered hacks triggered
        // from the input listener
        if !matches!(
            event,
            WindowManagerEvent::Show(SystemNotification::Manual(_), _)
        ) {
            tracing::info!(
                "processing event: {event} for process {} with notification {}",
                event.process_id(),
                event.notification(),
            );
        } else {
            tracing::trace!(
                "processing event: {event} for process {} with notification {}",
                event.process_id(),
                event.notification(),
            );
        }

        #[allow(clippy::useless_asref)]
        // We don't have From implemented for &mut WindowManager
        let initial_state = State::from(self.as_ref());

        self.enforce_workspace_rules()?;

        match event {
            WindowManagerEvent::FocusChange(notification, process_id, _) => {
                let activation_of_focused_application = is_activation_of_application(
                    notification,
                    self.focused_window()
                        .ok()
                        .map(|window| window.application.process_id),
                    process_id,
                );

                let application = self.application(process_id)?;
                let application_name = application.name().unwrap_or_default().clone();
                let mut should_switch_workspace_layer_to_tiling = true;
                let mut tabbed_window = false;
                let mut needs_reconciliation = false;

                // An application that komorebi brings to the front says so some time later, often
                // after komorebi has moved on to another. Moving focus or workspace to follow that
                // is moving it back to where it was, so it is only followed while the application
                // is still the one in front.
                let frontmost = MacosApi::frontmost_process_id();
                let stale = is_stale_focus_notification(frontmost, process_id);
                if stale {
                    tracing::warn!(
                        "not following {notification} for process {process_id}: process {} is in front",
                        frontmost.unwrap_or_default()
                    );
                }

                if let Some(window_id) = application.main_window_id()
                    && let Some(element) = application.main_window()
                {
                    let workspace = self.focused_workspace_mut()?;

                    let tabbed_applications = TABBED_APPLICATIONS.lock();
                    if tabbed_applications.contains(&application_name) {
                        let mut first_tab_destroyed = false;
                        let mut container_idx_to_update = None;

                        for (container_idx, container) in workspace.containers().iter().enumerate()
                        {
                            if let Some(window) = container.focused_window()
                                && window.application.name().unwrap_or_default() == application_name
                                && window.application.process_id == process_id
                            {
                                let tab_rect = MacosApi::window_rect(&element)?;
                                let main_rect = match MacosApi::window_rect(&window.element) {
                                    Ok(rect) => rect,
                                    Err(AccessibilityError::Api(
                                        AccessibilityApiError::InvalidUIElement,
                                    )) => {
                                        // this means we have closed the 1st tab, so we need this window object to be reaped
                                        // and for the new 1st tab to be the key element of the window struct
                                        if let Some(event) =
                                            WindowManagerEvent::from_system_notification(
                                                SystemNotification::Manual(
                                                    ManualNotification::ShowOnFocusChangeFirstTabDestroyed,
                                                ),
                                                event.process_id(),
                                                Some(window_id),
                                            )
                                        {
                                            window_manager_event_listener::send_notification(event);
                                        }

                                        first_tab_destroyed = true;

                                        tracing::debug!(
                                            "first tab of a native tabbed app was destroyed; reaping window and sending a new show event"
                                        );

                                        tab_rect
                                    }
                                    Err(error) => return Err(error.into()),
                                };

                                // Check if this is truly a tab change within the same window
                                // by verifying the stored element's window ID matches the container's window ID
                                if tab_rect == main_rect && window.id != window_id {
                                    // Additional check: verify the stored element belongs to this container
                                    // This prevents treating separate windows as tabs when they have matching geometry
                                    if let Ok(stored_window_id) =
                                        AccessibilityApi::window_id(&window.element.0)
                                        && stored_window_id == window.id
                                    {
                                        // All checks pass: stored element matches container ID = true tab change
                                        container_idx_to_update = Some(container_idx);
                                        tabbed_window = true;
                                    }
                                    // If stored_window_id != window.id, this is a stale element or different window
                                    // Don't set tabbed_window=true, allow normal focus handling
                                }
                                // If window.id == window_id, proceed with normal focus handling
                            }
                        }

                        // Update the window element outside the immutable borrow
                        if let Some(container_idx) = container_idx_to_update
                            && let Some(container) =
                                workspace.containers_mut().get_mut(container_idx)
                            && let Some(window) = container.focused_window_mut()
                        {
                            window.id = window_id;
                            window.element = AccessibilityUiElement(element.clone());
                        }

                        // check monocle_container for tabbed applications
                        let mut update_monocle = false;
                        if let Some(monocle) = &workspace.monocle_container
                            && let Some(window) = monocle.focused_window()
                            && window.application.name().unwrap_or_default() == application_name
                            && window.application.process_id == process_id
                        {
                            let tab_rect = MacosApi::window_rect(&element)?;
                            let main_rect = match MacosApi::window_rect(&window.element) {
                                Ok(rect) => rect,
                                Err(AccessibilityError::Api(
                                    AccessibilityApiError::InvalidUIElement,
                                )) => {
                                    first_tab_destroyed = true;
                                    tab_rect
                                }
                                Err(error) => return Err(error.into()),
                            };

                            if tab_rect == main_rect && window.id != window_id {
                                update_monocle = true;
                                tabbed_window = true;
                            }
                        }

                        if update_monocle
                            && let Some(monocle) = workspace.monocle_container.as_mut()
                            && let Some(window) = monocle.focused_window_mut()
                        {
                            window.id = window_id;
                            window.element = AccessibilityUiElement(element.clone());
                        }

                        if first_tab_destroyed {
                            self.reap_invalid_windows_for_application(process_id)?;
                        }
                    }

                    drop(tabbed_applications);

                    if !tabbed_window {
                        let is_known = self.known_window_ids.get(&window_id).cloned();
                        let mut is_on_current_workspace = false;

                        // TODO: figure out if this applies on macOS too
                        // don't want to trigger the full workspace updates when there are no managed
                        // containers - this makes floating windows on empty workspaces go into very
                        // annoying focus change loops which prevents users from interacting with them
                        if !matches!(
                            self.focused_workspace()?.layout,
                            Layout::Default(DefaultLayout::Scrolling)
                        ) && !self.focused_workspace()?.containers().is_empty()
                        {
                            self.update_focused_workspace(self.mouse_follows_focus, false)?;
                        }

                        let workspace = self.focused_workspace_mut()?;
                        if workspace.contains_window(window_id) {
                            is_on_current_workspace = true;
                        }

                        let floating_window_idx = workspace
                            .floating_windows()
                            .iter()
                            .position(|w| w.id == window_id);

                        match floating_window_idx {
                            None => {
                                // if let Some(w) = workspace.maximized_window() {
                                //     if w.hwnd == window_id {
                                //         return Ok(());
                                //     }
                                // }

                                if let Some(monocle) = &workspace.monocle_container {
                                    if let Some(window) = monocle.focused_window() {
                                        window.focus(false)?;
                                    }
                                } else if !is_on_current_workspace && is_known.is_none() {
                                    // thanks, I hate it - need to do this so that we don't mess up
                                    // the workspace rules, but also don't miss events from dumb
                                    // apps like notes, mail etc.
                                    let mut has_matching_workspace_rule = false;
                                    let workspace_rules = WORKSPACE_MATCHING_RULES.lock();
                                    for rule in &*workspace_rules {
                                        match &rule.matching_rule {
                                            MatchingRule::Simple(r) => {
                                                if r.id.trim_end_matches(".exe") == application_name
                                                {
                                                    has_matching_workspace_rule = true;
                                                }
                                            }
                                            // TODO: this is pretty coarse
                                            MatchingRule::Composite(rules) => {
                                                for r in rules {
                                                    if r.id.trim_end_matches(".exe")
                                                        == application_name
                                                    {
                                                        has_matching_workspace_rule = true;
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    if !has_matching_workspace_rule
                                        && workspace.focus_container_by_window(window_id).is_err()
                                    {
                                        // if this fails, the app was probably open but windowless when komorebi
                                        // launched, so the window hasn't been registered - we should treat it
                                        // as a "Show" event
                                        if let Some(event) =
                                                WindowManagerEvent::from_system_notification(
                                                    SystemNotification::Manual(
                                                        ManualNotification::ShowOnFocusChangeWindowlessAppRestored,
                                                    ),
                                                    event.process_id(),
                                                    Some(window_id),
                                                )
                                            {
                                                window_manager_event_listener::send_notification(
                                                    event,
                                                );
                                                should_switch_workspace_layer_to_tiling = false;
                                            }
                                    }
                                } else if should_refocus_container(
                                    stale,
                                    is_on_current_workspace,
                                    activation_of_focused_application,
                                ) {
                                    workspace.focus_container_by_window(window_id)?;
                                }

                                if should_switch_workspace_layer_to_tiling {
                                    workspace.layer = WorkspaceLayer::Tiling;
                                }

                                if matches!(
                                    self.focused_workspace()?.layout,
                                    Layout::Default(DefaultLayout::Scrolling)
                                ) && !self.focused_workspace()?.containers().is_empty()
                                {
                                    self.update_focused_workspace(self.mouse_follows_focus, false)?;
                                }
                            }
                            Some(idx) => {
                                if let Some(_window) = workspace.floating_windows().get(idx) {
                                    workspace.layer = WorkspaceLayer::Floating;
                                }
                            }
                        }

                        if should_follow_focus_to_workspace(
                            stale,
                            is_on_current_workspace,
                            is_known.is_some(),
                            activation_of_focused_application,
                        ) && let Some((m_idx, w_idx)) = is_known
                        {
                            workspace_reconciliator::send_notification(m_idx, w_idx, event);
                            needs_reconciliation = true;
                        }
                    }
                } else {
                    // An application whose window is parked on a hidden workspace can have no main
                    // window to ask for when it comes to the front, but the notification may still
                    // say which window it is about, and that is enough to follow it
                    tracing::debug!(
                        "{notification} for process {process_id}: no main window to ask for"
                    );

                    if let Some(window_id) = event.window_id()
                        && let Some((m_idx, w_idx)) = self.known_window_ids.get(&window_id).cloned()
                        && should_follow_focus_to_workspace(
                            stale,
                            self.focused_workspace()?.contains_window(window_id),
                            true,
                            activation_of_focused_application,
                        )
                    {
                        workspace_reconciliator::send_notification(m_idx, w_idx, event);
                        needs_reconciliation = true;
                    }
                }

                if matches!(
                    notification,
                    SystemNotification::Accessibility(
                        AccessibilityNotification::AXMainWindowChanged
                    )
                ) && !tabbed_window
                    && !needs_reconciliation
                {
                    self.reap_invalid_windows_for_application(process_id)?;
                    self.update_focused_workspace(false, false)?;
                }
            }
            // TODO: update this to work with floating applications / rules
            WindowManagerEvent::Show(_, process_id)
            | WindowManagerEvent::Manage(_, process_id, _) => {
                let focused_monitor_idx = self.focused_monitor_idx();
                let focused_workspace_idx =
                    self.focused_workspace_idx_for_monitor_idx(focused_monitor_idx)?;

                let mut window_id = None;
                let mut window_element = None;
                let mut application_name = String::new();
                let mut tabbed_window = false;
                let mut create = true;

                {
                    let application = self.application(process_id)?;
                    if let Some(element) = application.main_window()
                        && let Ok(wid) = AccessibilityApi::window_id(&element)
                    {
                        window_id = Some(wid);
                        window_element = Some(element.clone());
                        application_name = application.name().unwrap_or_default().clone();
                    }
                }

                if let (Some(window_id), Some(element)) = (window_id, &window_element) {
                    let workspace = self.focused_workspace()?;

                    let tabbed_applications = TABBED_APPLICATIONS.lock();
                    if tabbed_applications.contains(&application_name) {
                        // Get the window_id for the new window
                        if let Ok(new_window_id) = AccessibilityApi::window_id(element) {
                            for window in workspace.visible_windows().iter().flatten() {
                                if window.application.name().unwrap_or_default() == application_name
                                {
                                    let tab_rect = MacosApi::window_rect(element)?;
                                    let main_rect = MacosApi::window_rect(&window.element)?;
                                    // BOTH conditions must be true:
                                    // 1. Geometry matches (original check - preserves existing behavior)
                                    // 2. Window IDs match (new check - prevents false positives)
                                    if tab_rect == main_rect && window.id == new_window_id {
                                        tabbed_window = true;
                                        break;
                                    }
                                }
                            }
                        }
                    }

                    drop(tabbed_applications);

                    if workspace.contains_window(window_id) {
                        if !matches!(
                            event,
                            WindowManagerEvent::Show(SystemNotification::Manual(_), _)
                        ) {
                            // don't want to spam logs for manually triggered hacks triggered
                            // from the input listener
                            tracing::debug!("ignoring show event for window already on workspace");
                        }

                        // ignore bogus show events
                        create = false;
                    }

                    if let Some((m_idx, w_idx)) = self.known_window_ids.get(&window_id)
                        && let Ok(focused_workspace_idx) = self.focused_workspace_idx()
                        && (*m_idx != self.focused_monitor_idx() || *w_idx != focused_workspace_idx)
                    {
                        tracing::debug!(
                            "ignoring show event for window already associated with another workspace"
                        );

                        // TODO: probably shouldn't default here
                        AdhocWindow::hide(window_id, element, WindowHidingPosition::default())?;
                        create = false;
                    }
                }

                // this happens sometimes because of the mouse event from input listener which emits a show
                // before a window has updated things like its subrole, so we need to check again
                let application = self.application(process_id)?;
                if let Some(element) = &window_element
                    && let Ok(window) = Window::new(element.clone(), application.clone())
                    && !window.should_manage(Some(event), &mut rule_debug)?
                {
                    create = false;
                }

                if create
                    && !tabbed_window
                    && let Some(element) = window_element
                    && let Ok(mut window) = Window::new(element, application.clone())
                {
                    window.observe(&self.run_loop, None)?;
                    let behaviour = self
                        .window_management_behaviour(focused_monitor_idx, focused_workspace_idx);
                    let workspace = self.focused_workspace_mut()?;
                    let workspace_contains_window = workspace.contains_window(window.id);
                    let monocle_container = workspace.monocle_container.clone();

                    let floating_applications = FLOATING_APPLICATIONS.lock();
                    let mut should_float = false;

                    if !floating_applications.is_empty() {
                        let regex_identifiers = REGEX_IDENTIFIERS.lock();

                        if let (
                            Some(title),
                            Some(exe_name),
                            Some(role),
                            Some(subrole),
                            Some(path),
                        ) = (
                            window.title(),
                            window.exe(),
                            window.role(),
                            window.subrole(),
                            window.path(),
                        ) {
                            should_float = should_act(
                                &title,
                                &exe_name,
                                &[&role, &subrole],
                                &path.to_string_lossy(),
                                &floating_applications,
                                &regex_identifiers,
                            )
                            .is_some();
                        }
                    }

                    if behaviour.float_override
                        || behaviour.floating_layer_override
                        || (should_float && !matches!(event, WindowManagerEvent::Manage(_, _, _)))
                    {
                        let placement = if behaviour.floating_layer_override {
                            // Floating layer override placement
                            behaviour.floating_layer_placement
                        } else if behaviour.float_override {
                            // Float override placement
                            behaviour.float_override_placement
                        } else {
                            // Float rule placement
                            behaviour.float_rule_placement
                        };
                        // Center floating windows according to the proper placement if not
                        // on a floating workspace
                        let center_spawned_floats = placement.should_center() && workspace.tile;
                        workspace.floating_windows_mut().push_back(window.clone());
                        workspace.layer = WorkspaceLayer::Floating;
                        if center_spawned_floats {
                            let mut floating_window = window.clone();
                            floating_window
                                .center(&workspace.globals.work_area, placement.should_resize())?;
                        }

                        self.update_focused_workspace(false, false)?;
                    } else {
                        match behaviour.current_behaviour {
                            WindowContainerBehaviour::Create => {
                                workspace.new_container_for_window(&window)?;
                                workspace.layer = WorkspaceLayer::Tiling;
                                self.update_focused_workspace(false, false)?;
                            }
                            WindowContainerBehaviour::Append => {
                                let window_hiding_position =
                                    workspace.globals.window_hiding_position;
                                workspace
                                    .focused_container_mut()
                                    .ok_or_eyre("there is no focused container")?
                                    .add_window(&window, window_hiding_position)?;
                                workspace.layer = WorkspaceLayer::Tiling;
                                self.update_focused_workspace(true, false)?;
                            }
                        }

                        // TODO: not sure if this is needed on macOS
                        if (self.focused_workspace()?.containers().len() == 1
                            && self.focused_workspace()?.floating_windows().is_empty())
                            || (self.focused_workspace()?.containers().is_empty()
                                && self.focused_workspace()?.floating_windows().len() == 1)
                        {
                            // If after adding this window the workspace only contains 1 window, it
                            // means it was previously empty and we focused the desktop to unfocus
                            // any previous window from other workspace, so now we need to focus
                            // this window again. This is needed because sometimes some windows
                            // first send the `FocusChange` event and only the `Show` event after
                            // and we will be focusing the desktop on the `FocusChange` event since
                            // it is still empty.
                            window.focus(self.mouse_follows_focus)?;
                        }
                    }

                    if workspace_contains_window {
                        let mut monocle_window_event = false;
                        if let Some(ref monocle) = monocle_container
                            && let Some(monocle_window) = monocle.focused_window()
                        {
                            // we should have the window_id at this point
                            if monocle_window.id == window_id.unwrap_or_default() {
                                monocle_window_event = true;
                            }
                        }

                        let workspace = self.focused_workspace()?;
                        if !(monocle_window_event || workspace.layer != WorkspaceLayer::Tiling)
                            && monocle_container.is_some()
                        {
                            window.hide(workspace.globals.window_hiding_position)?;
                        }
                    }

                    // let workspace = self.focused_workspace_mut()?;
                    // workspace.new_container_for_window(&window)?;
                    //
                    // self.update_focused_workspace(false, false)?;
                }
            }
            WindowManagerEvent::Destroy(notification, process_id) => {
                // a new process may be given this id, and must not inherit what we know about it
                if matches!(
                    notification,
                    SystemNotification::AppKitWorkspace(
                        AppKitWorkspaceNotification::NSWorkspaceDidTerminateApplicationNotification
                    )
                ) {
                    enhanced_ui::forget(process_id);
                }

                // some apps like Discord in all of their relentless Electron slop stupidity
                // hijack CMD+W to send a HIDE instead of a close (i.e. the equivalent of pressing CMD+H)
                let mut should_force_reap = false;
                if matches!(
                    notification,
                    SystemNotification::Accessibility(
                        AccessibilityNotification::AXApplicationHidden
                    )
                ) {
                    let application = self.application(process_id)?;
                    let window_count = application
                        .window_elements()
                        .map(|elements| elements.len())
                        .unwrap_or(0);

                    // force reap if the app has exactly 1 window (i.e. the one being hidden)
                    if window_count == 1 {
                        tracing::debug!(
                            "app {} has only 1 window and sent AXApplicationHidden, treating as window close",
                            application.name().unwrap_or_default()
                        );
                        should_force_reap = true;
                    }
                }

                if should_force_reap {
                    let workspace = self.focused_workspace_mut()?;
                    workspace.reap_invalid_windows_for_application(process_id, &[])?;
                } else {
                    // app has multiple windows, use normal orphan reaping strategy
                    self.reap_invalid_windows_for_application(process_id)?;
                }

                // if the workspace is now empty (last window was closed), activate Finder
                // instead of letting an app on another workspace take focus (technically
                // the other app will take focus first, but this ensures that _eventually_
                // i.e. quicker than the user can recognize, Finder will be the focused app)
                if self.focused_workspace()?.containers().is_empty() {
                    tracing::debug!(
                        "workspace is now empty, activating Finder to prevent unwanted workspace switch"
                    );
                    MacosApi::activate_finder();
                }

                self.update_focused_workspace(false, false)?;
            }
            WindowManagerEvent::Unmanage(_, _, window_id) => {
                let behaviour = self.window_management_behaviour(
                    self.focused_monitor_idx(),
                    self.focused_workspace_idx()?,
                );

                let workspace = self.focused_workspace_mut()?;
                let mut window = workspace.remove_window(window_id)?;

                // If we unmanaged a window, it shouldn't be immediately hidden behind managed windows
                let placement = if behaviour.floating_layer_override {
                    // Floating layer override placement
                    behaviour.floating_layer_placement
                } else if behaviour.float_override {
                    // Float override placement
                    behaviour.float_override_placement
                } else {
                    // Float rule placement
                    behaviour.float_rule_placement
                };

                window.center(&workspace.globals.work_area, placement.should_resize())?;

                // we only want to add the window ID after the window has been removed
                UNMANAGED_WINDOW_IDS.lock().push(window_id);

                self.update_focused_workspace(false, false)?;
            }
            WindowManagerEvent::Minimize(_, _, window_id) => {
                self.extract_minimized_window(window_id)?;
                self.update_focused_workspace(false, false)?;
            }
            WindowManagerEvent::Restore(_, _, window_id) => {
                match self.minimized_windows.remove(&window_id) {
                    None => {}
                    Some(window) => {
                        let workspace = self.focused_workspace_mut()?;
                        workspace.new_container_for_window(&window)?;
                        self.update_focused_workspace(false, false)?;
                    }
                }
            }
            WindowManagerEvent::MoveStart(_, _, window_id) => {
                // somebody is dragging the window, so we must stop animating it somewhere else
                animator::cancel(window_id);

                if self.pending_move_op.is_none() && self.pending_resize_op.is_none() {
                    let monitor_idx = self.focused_monitor_idx();
                    let workspace_idx = self
                        .focused_monitor()
                        .ok_or_eyre("there is no monitor with this idx")?
                        .focused_workspace_idx();

                    let pending_move_op = Arc::make_mut(&mut self.pending_move_op);
                    *pending_move_op = Option::from((monitor_idx, workspace_idx, window_id));
                }
            }
            WindowManagerEvent::MoveEnd(_, _, window_id) => {
                // We need this because if the event ends on a different monitor,
                // that monitor will already have been focused and updated in the state
                let pending = *self.pending_move_op;
                // Always consume the pending move op whenever this event is handled
                let pending_move_op = Arc::make_mut(&mut self.pending_move_op);
                *pending_move_op = None;

                // The window of a drag by an edge that the windows next to it follow is where the
                // layout has put its tile, and not where somebody has moved it to, so it is not a
                // move that it ends
                let following_drag = self
                    .live_resize
                    .as_ref()
                    .is_some_and(|live| live.window == window_id);

                if let Some((origin_monitor_idx, origin_workspace_idx, wid)) = pending
                    && !following_drag
                {
                    // If the window handles don't match then something went wrong and the pending move
                    // is not related to this current move, if so abort this operation.
                    if wid != window_id {
                        eyre::bail!(
                            "window handles for move operation don't match: {} != {}",
                            wid,
                            window_id
                        );
                    }
                    let known_window_ids = self.known_window_ids.clone();

                    let target_monitor_idx = self
                        .monitor_idx_from_current_pos()
                        .ok_or_eyre("cannot get monitor idx from current position")?;

                    let focused_monitor_idx = self.focused_monitor_idx();
                    let focused_workspace_idx = self.focused_workspace_idx().unwrap_or_default();
                    let window_management_behaviour = self
                        .window_management_behaviour(focused_monitor_idx, focused_workspace_idx);

                    let workspace = self.focused_workspace_mut()?;
                    let focused_container_idx = workspace.focused_container_idx();

                    if let Some(container) = workspace.focused_container()
                        && let Some(window) = container.focused_window()
                    {
                        // TODO: not sure about this clone
                        let window = window.clone();
                        let new_position = Rect::from(MacosApi::window_rect(&window.element)?);
                        let old_position = *workspace
                            .latest_layout
                            .get(focused_container_idx)
                            // If the move was to another monitor with an empty workspace, the
                            // workspace here will refer to that empty workspace, which won't
                            // have any latest layout set. We fall back to a Default for Rect
                            // which allows us to make a reasonable guess that the drag has taken
                            // place across a monitor boundary to an empty workspace
                            .unwrap_or(&Rect::default());

                        // This will be true if we have moved to another monitor
                        let mut moved_across_monitors = false;

                        if let Some((m_idx, _)) = known_window_ids.get(&window_id)
                            && *m_idx != target_monitor_idx
                        {
                            moved_across_monitors = true;
                        }

                        // If we didn't move to another monitor with an empty workspace, it is
                        // still possible that we moved to another monitor with a populated workspace
                        if !moved_across_monitors {
                            // So we'll check if the origin monitor index and the target monitor index
                            // are different, if they are, we can set the override
                            moved_across_monitors = origin_monitor_idx != target_monitor_idx;

                            if moved_across_monitors {
                                // Want to make sure that we exclude unmanaged windows from cross-monitor
                                // moves with a mouse, otherwise the currently focused idx container will
                                // be moved when we just want to drag an unmanaged window
                                let origin_workspace = self
                                    .monitors()
                                    .get(origin_monitor_idx)
                                    .ok_or_eyre("cannot get monitor idx")?
                                    .workspaces()
                                    .get(origin_workspace_idx)
                                    .ok_or_eyre("cannot get workspace idx")?;

                                let managed_window = origin_workspace.contains_window(window_id);

                                if !managed_window {
                                    moved_across_monitors = false;
                                }
                            }
                        }

                        let workspace = self.focused_workspace_mut()?;
                        if (workspace.tile && workspace.contains_managed_window(window_id))
                            || moved_across_monitors
                        {
                            let resize = Rect {
                                left: new_position.left - old_position.left,
                                top: new_position.top - old_position.top,
                                right: new_position.right - old_position.right,
                                bottom: new_position.bottom - old_position.bottom,
                            };

                            // If we have moved across the monitors, use that override, otherwise determine
                            // if a move has taken place by ruling out a resize
                            let right_bottom_constant = 0;

                            let is_move = moved_across_monitors
                                || resize.right.abs() == right_bottom_constant
                                    && resize.bottom.abs() == right_bottom_constant;

                            if is_move {
                                tracing::info!("moving with mouse");

                                if moved_across_monitors {
                                    if let Some((
                                        origin_monitor_idx,
                                        origin_workspace_idx,
                                        w_hwnd,
                                    )) = pending
                                    {
                                        let target_workspace_idx = self
                                            .monitors()
                                            .get(target_monitor_idx)
                                            .ok_or_eyre("there is no monitor at this idx")?
                                            .focused_workspace_idx();

                                        let target_container_idx = self
                                            .monitors()
                                            .get(target_monitor_idx)
                                            .ok_or_eyre("there is no monitor at this idx")?
                                            .focused_workspace()
                                            .ok_or_eyre(
                                                "there is no focused workspace for this monitor",
                                            )?
                                            .container_idx_from_current_point()
                                            // Default to 0 in the case of an empty workspace
                                            .unwrap_or(0);

                                        let origin =
                                            (origin_monitor_idx, origin_workspace_idx, w_hwnd);
                                        let target = (
                                            target_monitor_idx,
                                            target_workspace_idx,
                                            target_container_idx,
                                        );
                                        self.transfer_window(origin, target)?;

                                        // We want to make sure both the origin and target monitors are updated,
                                        // so that we don't have ghost tiles until we force an interaction on
                                        // the origin monitor's focused workspace
                                        self.focus_monitor(origin_monitor_idx)?;
                                        let origin_monitor = self
                                            .monitors_mut()
                                            .get_mut(origin_monitor_idx)
                                            .ok_or_eyre("there is no monitor at this idx")?;
                                        origin_monitor.focus_workspace(origin_workspace_idx)?;
                                        self.update_focused_workspace(false, false)?;

                                        self.focus_monitor(target_monitor_idx)?;
                                        let target_monitor = self
                                            .monitors_mut()
                                            .get_mut(target_monitor_idx)
                                            .ok_or_eyre("there is no monitor at this idx")?;
                                        target_monitor.focus_workspace(target_workspace_idx)?;
                                        self.update_focused_workspace(false, false)?;

                                        // Make sure to give focus to the moved window again
                                        window.focus(self.mouse_follows_focus)?;
                                    }
                                } else if window_management_behaviour.float_override {
                                    // TODO: unsure of this clone
                                    workspace.floating_windows_mut().push_back(window);
                                    self.update_focused_workspace(false, false)?;
                                } else {
                                    match window_management_behaviour.current_behaviour {
                                        WindowContainerBehaviour::Create => {
                                            match workspace.container_idx_from_current_point() {
                                                Some(target_idx) => {
                                                    workspace.swap_containers(
                                                        focused_container_idx,
                                                        target_idx,
                                                    );
                                                    self.update_focused_workspace(false, false)?;
                                                }
                                                None => {
                                                    self.update_focused_workspace(
                                                        self.mouse_follows_focus,
                                                        false,
                                                    )?;
                                                }
                                            }
                                        }
                                        WindowContainerBehaviour::Append => {
                                            match workspace.container_idx_from_current_point() {
                                                Some(target_idx) => {
                                                    workspace
                                                        .move_window_to_container(target_idx)?;
                                                    self.update_focused_workspace(false, false)?;
                                                }
                                                None => {
                                                    self.update_focused_workspace(
                                                        self.mouse_follows_focus,
                                                        false,
                                                    )?;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            WindowManagerEvent::ResizeStart(_, _, window_id) => {
                // While an edge is dragged the windows next to it are laid out again and again,
                // and they say that they were resized, which is not somebody dragging them. Most
                // of that is dropped where it is heard, but what was heard before the drag was
                // followed is dropped here, since taking it for a drag would lose the one that
                // is going on.
                if self.another_window_is_being_dragged(window_id) {
                    tracing::debug!(
                        "ignoring the start of a resize of window {window_id}: it is following a drag"
                    );
                } else {
                    // a drag that was followed, and whose end was never heard
                    if self
                        .live_resize
                        .as_ref()
                        .is_some_and(|live| live.window != window_id)
                    {
                        self.live_resize_forget();
                    }

                    animator::cancel(window_id);

                    // AXWindowResized fires again and again through a drag; only the first frame is
                    // the one the drag started from
                    let already_recorded = matches!(
                        *self.pending_resize_op,
                        Some((wid, Some(_))) if wid == window_id
                    );

                    if !already_recorded {
                        let workspace = self.focused_workspace()?;
                        if let Some(container) = workspace.container_for_window(window_id)
                            && let Some(window) =
                                container.windows().iter().find(|w| w.id == window_id)
                        {
                            let started_from = Rect::from(MacosApi::window_rect(&window.element)?);

                            let pending_resize_op = Arc::make_mut(&mut self.pending_resize_op);
                            *pending_resize_op = Option::from((window_id, Some(started_from)));

                            // the windows next to the edge follow it from here
                            self.live_resize_begin(window_id);

                            // the edges at the left and at the top move the window as well, which
                            // is somebody resizing it, and not moving it
                            if self.live_resize.is_some()
                                && matches!(
                                    *self.pending_move_op,
                                    Some((_, _, wid)) if wid == window_id
                                )
                            {
                                let pending_move_op = Arc::make_mut(&mut self.pending_move_op);
                                *pending_move_op = None;
                            }
                        }
                    }
                }
            }
            WindowManagerEvent::ResizeEnd(_, _, window_id) => {
                // The drag is over, and so is anything that followed it. The windows next to the
                // edge are what the layout was when it began, for the end to be worked out from
                // there, as it is when there were no steps.
                let dragged = self.live_resize.as_ref().map(|live| live.window);
                let stepped = self.live_resize_end();

                let pending = *self.pending_resize_op;
                // Always consume the pending resize op whenever this event is handled
                let pending_resize_op = Arc::make_mut(&mut self.pending_resize_op);
                *pending_resize_op = None;

                // The end of a drag that was followed is the end of that drag, whichever window
                // the event is about: the windows next to the edge say that they were resized as
                // the button goes up, and it is the window that was dragged that is looked at.
                let window_id = match (dragged, pending) {
                    (Some(dragged), Some((wid, _))) if wid == dragged => dragged,
                    _ => window_id,
                };

                // the windows next to the edge are where the last step put them, a few pixels from
                // where the end of the drag does, which is not worth animating
                let _snapping = stepped.then(Snapping::end);

                let mut redraw = stepped;
                let mut mismatched = None;

                if let Some((wid, Some(started_from))) = pending {
                    if wid != window_id {
                        mismatched = Some(wid);
                    } else {
                        let workspace = self.focused_workspace()?;
                        if workspace.tile
                            && let Some(container) = workspace.container_for_window(window_id)
                            && let Some(window) =
                                container.windows().iter().find(|w| w.id == window_id)
                        {
                            let ended_at = Rect::from(MacosApi::window_rect(&window.element)?);

                            tracing::info!("resizing with mouse");
                            redraw |= self.resize_by_drag(window_id, started_from, ended_at)?;
                        }
                    }
                }

                if redraw {
                    self.update_focused_workspace(false, false)?;
                }

                // If the window handles don't match then something went wrong and the pending resize
                // is not related to this current resize, if so abort this operation.
                if let Some(wid) = mismatched {
                    eyre::bail!(
                        "window handles for resize operation don't match: {} != {}",
                        wid,
                        window_id
                    );
                }
            }
            // handled before this match
            WindowManagerEvent::ResizeStep(_, _, _)
            | WindowManagerEvent::SpaceChange(_, _)
            | WindowManagerEvent::ScreenLock(_, _)
            | WindowManagerEvent::ScreenUnlock(_, _) => {}
        }

        self.update_known_window_ids();

        notify_subscribers(
            Notification {
                event: NotificationEvent::WindowManager(event),
                state: self.as_ref().into(),
            },
            initial_state.has_been_modified(self.as_ref()),
        )?;

        border_manager::send_notification(window_element, window_id, false);

        Ok(())
    }
}

/// Whether a notification that the application with this process id came to the front is out of
/// date, which it is if some other application is in front now.
///
/// An application can be told to come to the front, and a while later tell komorebi that it did,
/// by which time another one has been told to, so what it says is no longer true. Nothing is out
/// of date if what is in front is not known, which leaves things as they were.
fn is_stale_focus_notification(frontmost: Option<i32>, process_id: i32) -> bool {
    frontmost.is_some_and(|frontmost| frontmost != process_id)
}

/// Whether a notification says that an application came to the front, when the window that is
/// focused here is one of its own.
///
/// The application reports its main window, which is not necessarily the one komorebi focused: it
/// can be another of its windows, parked on another workspace or beside the focused one. That is
/// komorebi's own focus coming back, not a request to move it, unlike a window of the application
/// being chosen, which is said with `AXMainWindowChanged`.
fn is_activation_of_application(
    notification: SystemNotification,
    focused_window_process_id: Option<i32>,
    process_id: i32,
) -> bool {
    matches!(
        notification,
        SystemNotification::AppKitWorkspace(
            AppKitWorkspaceNotification::NSWorkspaceDidActivateApplicationNotification
        ) | SystemNotification::Accessibility(AccessibilityNotification::AXApplicationActivated)
    ) && focused_window_process_id == Some(process_id)
}

/// Whether an application coming to the front should take komorebi to the workspace that has the
/// window it names. Not when the notification is stale, nor when the window is already on the
/// workspace that is showing, nor when it is only the application saying which of its windows is
/// its main one.
fn should_follow_focus_to_workspace(
    stale: bool,
    is_on_current_workspace: bool,
    window_is_known: bool,
    activation_of_focused_application: bool,
) -> bool {
    !stale && !is_on_current_workspace && window_is_known && !activation_of_focused_application
}

/// Whether the container of the window a notification names should take the focus, when the window
/// is on the workspace that is showing.
fn should_refocus_container(
    stale: bool,
    is_on_current_workspace: bool,
    activation_of_focused_application: bool,
) -> bool {
    is_on_current_workspace && !stale && !activation_of_focused_application
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_application_that_is_in_front_is_followed() {
        assert!(!is_stale_focus_notification(Some(79469), 79469));
    }

    #[test]
    fn an_application_that_is_not_in_front_any_more_is_not() {
        // Beeper says that it came to the front after Slack has
        assert!(is_stale_focus_notification(Some(96328), 79469));
    }

    #[test]
    fn nothing_is_stale_when_it_is_not_known_what_is_in_front() {
        assert!(!is_stale_focus_notification(None, 79469));
    }

    const ACTIVATED: SystemNotification = SystemNotification::AppKitWorkspace(
        AppKitWorkspaceNotification::NSWorkspaceDidActivateApplicationNotification,
    );
    const MAIN_WINDOW_CHANGED: SystemNotification =
        SystemNotification::Accessibility(AccessibilityNotification::AXMainWindowChanged);

    #[test]
    fn an_activation_of_the_application_with_the_focused_window_is_recognised() {
        // WezTerm (875) says it came to the front while one of its windows is focused here
        assert!(is_activation_of_application(ACTIVATED, Some(875), 875));
        assert!(is_activation_of_application(
            SystemNotification::Accessibility(AccessibilityNotification::AXApplicationActivated),
            Some(875),
            875
        ));
    }

    #[test]
    fn an_activation_of_another_application_is_not() {
        // Beeper (868) comes to the front while one of Zen's (833) windows is focused here
        assert!(!is_activation_of_application(ACTIVATED, Some(833), 868));
        assert!(!is_activation_of_application(ACTIVATED, None, 868));
    }

    #[test]
    fn choosing_a_window_of_the_focused_application_is_not_an_activation() {
        assert!(!is_activation_of_application(
            MAIN_WINDOW_CHANGED,
            Some(875),
            875
        ));
    }

    #[test]
    fn a_window_known_on_another_workspace_is_followed() {
        assert!(should_follow_focus_to_workspace(false, false, true, false));
    }

    #[test]
    fn a_stale_notification_is_not_followed() {
        assert!(!should_follow_focus_to_workspace(true, false, true, false));
    }

    #[test]
    fn a_window_on_the_workspace_that_is_showing_is_not_followed() {
        assert!(!should_follow_focus_to_workspace(false, true, true, false));
    }

    #[test]
    fn another_window_of_the_application_whose_window_is_focused_is_not_followed() {
        // WezTerm (875) is told to focus window 51 here and reports its main window, 939, which
        // is parked on another workspace: that is not a request to go there
        assert!(!should_follow_focus_to_workspace(false, false, true, true));
    }

    #[test]
    fn a_window_on_the_workspace_that_is_showing_takes_the_focus() {
        assert!(should_refocus_container(false, true, false));
    }

    #[test]
    fn a_stale_notification_does_not_take_the_focus() {
        assert!(!should_refocus_container(true, true, false));
    }

    #[test]
    fn a_window_that_is_not_on_the_workspace_that_is_showing_does_not_take_the_focus() {
        assert!(!should_refocus_container(false, false, false));
    }

    #[test]
    fn another_window_of_the_application_whose_window_is_focused_does_not_take_the_focus() {
        // WezTerm (875) is told to focus window 51 here and reports its main window, 939, which is
        // beside it on the same workspace: the focus stays on 51
        assert!(!should_refocus_container(false, true, true));
    }
}
