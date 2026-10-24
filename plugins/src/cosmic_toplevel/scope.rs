// SPDX-License-Identifier: GPL-3.0-only
// Copyright © 2026 System76

//! Decide which windows a window switcher lists for a [`WorkspaceScope`],
//! and for a same-app (Alt+`) switcher, which of those share the focused app.
//!
//! Kept free of Wayland types so it can be tested directly. The rules mirror
//! the App List applet's `filter_top_levels` in cosmic-applets: a window whose
//! workspace is unknown, or listed while the active workspaces are not known
//! yet, counts as being on the active workspace rather than disappearing.

use pop_launcher::WorkspaceScope;

/// Where a window is, reduced to what the scope filter needs.
#[derive(Debug, Clone)]
pub struct WindowPlacement<W, O> {
    /// Workspaces the window is on; empty when the compositor didn't say.
    pub workspaces: Vec<W>,
    /// Outputs the window is shown on; empty when the compositor didn't say.
    pub outputs: Vec<O>,
    /// Wayland app ID; empty when the client didn't set one.
    pub app_id: String,
    /// Whether this is the focused (activated) window.
    pub focused: bool,
}

/// Outputs of the focused window, or an empty list when nothing is focused.
pub fn focused_outputs<W, O: Clone>(windows: &[WindowPlacement<W, O>]) -> Vec<O> {
    windows
        .iter()
        .find(|window| window.focused)
        .map(|window| window.outputs.clone())
        .unwrap_or_default()
}

/// Outputs that "this display" refers to for a window switcher.
///
/// The output the frontend names wins, since it is where the switcher is
/// shown and so where the user is. Keyboard focus can linger on another
/// output after switching workspaces, so it is only the fallback, also used
/// when the name doesn't match any known output.
///
/// `named_outputs` pairs every known output with its name.
pub fn target_outputs<O: Clone>(
    requested: Option<&str>,
    named_outputs: &[(O, String)],
    focused_outputs: Vec<O>,
) -> Vec<O> {
    let requested: Vec<O> = named_outputs
        .iter()
        .filter(|(_, name)| Some(name.as_str()) == requested)
        .map(|(output, _)| output.clone())
        .collect();

    if requested.is_empty() {
        focused_outputs
    } else {
        requested
    }
}

/// Whether `window` belongs in a window switcher with the given `scope`.
///
/// `active_workspaces` holds the workspace currently shown on each output.
/// `target_outputs` is the result of [`target_outputs`]: the outputs that
/// [`WorkspaceScope::FocusedOutputWorkspace`] keeps windows on.
pub fn in_scope<W: PartialEq, O: PartialEq>(
    scope: WorkspaceScope,
    window: &WindowPlacement<W, O>,
    active_workspaces: &[W],
    target_outputs: &[O],
) -> bool {
    let on_active_workspace = active_workspaces.is_empty()
        || window.workspaces.is_empty()
        || window
            .workspaces
            .iter()
            .any(|workspace| active_workspaces.contains(workspace));

    match scope {
        WorkspaceScope::AllWorkspaces => true,
        WorkspaceScope::ActiveWorkspaces => on_active_workspace,
        WorkspaceScope::FocusedOutputWorkspace => {
            // Neither a named output nor a focused window: there is no "this
            // display" to pick, and an empty list makes cosmic-launcher skip
            // the switcher entirely.
            let on_target_output = !target_outputs.is_empty()
                && (window.outputs.is_empty()
                    || window
                        .outputs
                        .iter()
                        .any(|output| target_outputs.contains(output)));

            on_active_workspace && on_target_output
        }
    }
}

/// Indices of the windows a switcher lists, in the order of `windows`.
///
/// `windows` is in usage order, most recently used last.
///
/// Keeps the windows [`in_scope`] for `scope`. With `same_app`, it keeps only
/// the windows whose app ID matches a reference window's:
/// - the focused window;
/// - except on "this display only" when the focused window is on another
///   display. The display is the one the cursor is on, so the reference is
///   that display's most recently used window, the one the user would be in
///   there.
///
/// With `same_app`, it lists nothing when:
/// - there is no reference window, since there is no app to follow;
/// - its app ID is empty, since that doesn't identify an app;
/// - it would be the only entry, since switching to it changes nothing.
pub fn switcher_windows<W: PartialEq, O: PartialEq>(
    windows: &[WindowPlacement<W, O>],
    scope: WorkspaceScope,
    same_app: bool,
    active_workspaces: &[W],
    target_outputs: &[O],
) -> Vec<usize> {
    let listed =
        |window: &WindowPlacement<W, O>| in_scope(scope, window, active_workspaces, target_outputs);

    // The reference window's index and app ID, when only its app is listed.
    let focused_app = if same_app {
        let focused = windows.iter().position(|window| window.focused);
        let reference = match scope {
            WorkspaceScope::FocusedOutputWorkspace => focused
                .filter(|&index| listed(&windows[index]))
                .or_else(|| windows.iter().rposition(listed)),
            WorkspaceScope::AllWorkspaces | WorkspaceScope::ActiveWorkspaces => focused,
        };
        match reference {
            Some(index) if !windows[index].app_id.is_empty() => {
                Some((index, windows[index].app_id.as_str()))
            }
            _ => return Vec::new(),
        }
    } else {
        None
    };

    let picked: Vec<usize> = windows
        .iter()
        .enumerate()
        .filter(|(_, window)| focused_app.is_none_or(|(_, app_id)| window.app_id == app_id))
        .filter(|(_, window)| listed(window))
        .map(|(index, _)| index)
        .collect();

    match focused_app {
        Some((focused, _)) if picked == [focused] => Vec::new(),
        _ => picked,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pop_launcher::WorkspaceScope;

    // Two displays: workspace 1 is active on "A", workspace 5 on "B";
    // workspace 2 exists but isn't shown anywhere.
    const ACTIVE: &[u32] = &[1, 5];

    fn window(workspaces: &[u32], outputs: &[&'static str]) -> WindowPlacement<u32, &'static str> {
        app_window(workspaces, outputs, "app")
    }

    fn app_window(
        workspaces: &[u32],
        outputs: &[&'static str],
        app_id: &str,
    ) -> WindowPlacement<u32, &'static str> {
        WindowPlacement {
            workspaces: workspaces.to_vec(),
            outputs: outputs.to_vec(),
            app_id: app_id.to_string(),
            focused: false,
        }
    }

    fn focused_app_window(
        workspaces: &[u32],
        outputs: &[&'static str],
        app_id: &str,
    ) -> WindowPlacement<u32, &'static str> {
        WindowPlacement {
            focused: true,
            ..app_window(workspaces, outputs, app_id)
        }
    }

    #[test]
    fn all_workspaces_keeps_everything() {
        assert!(in_scope(
            WorkspaceScope::AllWorkspaces,
            &window(&[2], &["A"]),
            ACTIVE,
            &[]
        ));
    }

    #[test]
    fn active_workspaces_keeps_windows_on_any_active_workspace() {
        let scope = WorkspaceScope::ActiveWorkspaces;
        assert!(in_scope(scope, &window(&[1], &["A"]), ACTIVE, &[]));
        assert!(in_scope(scope, &window(&[5], &["B"]), ACTIVE, &[]));
        assert!(!in_scope(scope, &window(&[2], &["A"]), ACTIVE, &[]));
    }

    #[test]
    fn unknown_workspace_info_keeps_window() {
        let scope = WorkspaceScope::ActiveWorkspaces;
        assert!(in_scope(scope, &window(&[2], &["A"]), &[], &[]));
        assert!(in_scope(scope, &window(&[], &["A"]), ACTIVE, &[]));
    }

    #[test]
    fn focused_output_keeps_only_active_workspace_on_focused_display() {
        let scope = WorkspaceScope::FocusedOutputWorkspace;
        assert!(in_scope(scope, &window(&[1], &["A"]), ACTIVE, &["A"]));
        assert!(!in_scope(scope, &window(&[5], &["B"]), ACTIVE, &["A"]));
        assert!(!in_scope(scope, &window(&[2], &["A"]), ACTIVE, &["A"]));
    }

    #[test]
    fn focused_output_without_focused_window_keeps_nothing() {
        let scope = WorkspaceScope::FocusedOutputWorkspace;
        assert!(!in_scope(scope, &window(&[1], &["A"]), ACTIVE, &[]));
    }

    #[test]
    fn window_without_output_info_is_kept_on_focused_output() {
        let scope = WorkspaceScope::FocusedOutputWorkspace;
        assert!(in_scope(scope, &window(&[1], &[]), ACTIVE, &["A"]));
    }

    #[test]
    fn requested_output_wins_over_focused_window() {
        let named = [("A", "DP-1".to_string()), ("B", "HDMI-1".to_string())];
        assert_eq!(target_outputs(Some("HDMI-1"), &named, vec!["A"]), vec!["B"]);
    }

    #[test]
    fn unknown_requested_output_falls_back_to_focused_window() {
        let named = [("A", "DP-1".to_string())];
        assert_eq!(target_outputs(Some("DP-9"), &named, vec!["A"]), vec!["A"]);
    }

    #[test]
    fn no_requested_output_uses_focused_window() {
        let named = [("A", "DP-1".to_string()), ("B", "HDMI-1".to_string())];
        assert_eq!(target_outputs(None, &named, vec!["A"]), vec!["A"]);
    }

    #[test]
    fn focused_outputs_returns_outputs_of_activated_window() {
        let mut windows = vec![
            window(&[1], &["A"]),
            window(&[5], &["B"]),
            window(&[2], &["A"]),
        ];
        assert!(focused_outputs(&windows).is_empty());

        windows[1].focused = true;
        assert_eq!(focused_outputs(&windows), vec!["B"]);
    }

    #[test]
    fn without_same_app_keeps_every_window_in_scope() {
        let windows = [
            focused_app_window(&[1], &["A"], "firefox"),
            app_window(&[1], &["A"], "term"),
            app_window(&[2], &["A"], "firefox"),
        ];
        let picked = switcher_windows(&windows, WorkspaceScope::AllWorkspaces, false, ACTIVE, &[]);
        assert_eq!(picked, vec![0, 1, 2]);
    }

    #[test]
    fn same_app_keeps_only_the_focused_apps_windows() {
        let windows = [
            focused_app_window(&[1], &["A"], "firefox"),
            app_window(&[1], &["A"], "term"),
            app_window(&[1], &["A"], "firefox"),
        ];
        let picked = switcher_windows(&windows, WorkspaceScope::AllWorkspaces, true, ACTIVE, &[]);
        assert_eq!(picked, vec![0, 2]);
    }

    #[test]
    fn same_app_intersects_with_the_scope() {
        let windows = [
            focused_app_window(&[1], &["A"], "firefox"),
            app_window(&[2], &["A"], "firefox"),
            app_window(&[5], &["B"], "firefox"),
        ];
        let picked = switcher_windows(
            &windows,
            WorkspaceScope::ActiveWorkspaces,
            true,
            ACTIVE,
            &[],
        );
        assert_eq!(picked, vec![0, 2]);
    }

    #[test]
    fn same_app_without_focused_window_lists_nothing() {
        let windows = [
            app_window(&[1], &["A"], "firefox"),
            app_window(&[1], &["A"], "firefox"),
        ];
        let picked = switcher_windows(&windows, WorkspaceScope::AllWorkspaces, true, ACTIVE, &[]);
        assert!(picked.is_empty());
    }

    // An empty app ID says nothing about which app a window belongs to, so
    // two such windows must not be treated as the same app.
    #[test]
    fn same_app_with_empty_app_id_lists_nothing() {
        let windows = [
            focused_app_window(&[1], &["A"], ""),
            app_window(&[1], &["A"], ""),
        ];
        let picked = switcher_windows(&windows, WorkspaceScope::AllWorkspaces, true, ACTIVE, &[]);
        assert!(picked.is_empty());
    }

    #[test]
    fn same_app_with_only_the_focused_window_lists_nothing() {
        let windows = [
            focused_app_window(&[1], &["A"], "firefox"),
            app_window(&[1], &["A"], "term"),
        ];
        let picked = switcher_windows(&windows, WorkspaceScope::AllWorkspaces, true, ACTIVE, &[]);
        assert!(picked.is_empty());
    }

    // "This display only" follows the cursor's display: with focus elsewhere,
    // the app is that of the display's most recently used window, here the
    // last listed terminal, not the focused Firefox.
    #[test]
    fn this_display_follows_the_last_used_window_on_the_target_display() {
        let windows = [
            focused_app_window(&[1], &["A"], "firefox"),
            app_window(&[5], &["B"], "firefox"),
            app_window(&[5], &["B"], "term"),
            app_window(&[5], &["B"], "term"),
        ];
        let picked = switcher_windows(
            &windows,
            WorkspaceScope::FocusedOutputWorkspace,
            true,
            ACTIVE,
            &["B"],
        );
        assert_eq!(picked, vec![2, 3]);
    }

    #[test]
    fn this_display_with_the_focused_window_on_it_follows_the_focused_window() {
        let windows = [
            focused_app_window(&[5], &["B"], "firefox"),
            app_window(&[5], &["B"], "firefox"),
            app_window(&[5], &["B"], "term"),
        ];
        let picked = switcher_windows(
            &windows,
            WorkspaceScope::FocusedOutputWorkspace,
            true,
            ACTIVE,
            &["B"],
        );
        assert_eq!(picked, vec![0, 1]);
    }

    // The cursor's display has a single window of its last used app: that
    // window is already the one to be in, so there is nothing to switch to.
    #[test]
    fn this_display_with_a_lone_window_of_its_last_used_app_lists_nothing() {
        let windows = [
            focused_app_window(&[1], &["A"], "firefox"),
            app_window(&[5], &["B"], "firefox"),
        ];
        let picked = switcher_windows(
            &windows,
            WorkspaceScope::FocusedOutputWorkspace,
            true,
            ACTIVE,
            &["B"],
        );
        assert!(picked.is_empty());
    }

    #[test]
    fn this_display_with_an_empty_workspace_lists_nothing() {
        let windows = [
            focused_app_window(&[1], &["A"], "firefox"),
            app_window(&[1], &["A"], "firefox"),
        ];
        let picked = switcher_windows(
            &windows,
            WorkspaceScope::FocusedOutputWorkspace,
            true,
            ACTIVE,
            &["B"],
        );
        assert!(picked.is_empty());
    }
}
