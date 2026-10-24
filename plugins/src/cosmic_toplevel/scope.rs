// SPDX-License-Identifier: GPL-3.0-only
// Copyright © 2026 System76

//! Decide which windows a window switcher lists for a [`WorkspaceScope`].
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
/// `focused_outputs` is the result of [`target_outputs`]: the outputs that
/// [`WorkspaceScope::FocusedOutputWorkspace`] keeps windows on.
pub fn in_scope<W: PartialEq, O: PartialEq>(
    scope: WorkspaceScope,
    window: &WindowPlacement<W, O>,
    active_workspaces: &[W],
    focused_outputs: &[O],
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
            // Without a focused window there is no "this display" to pick;
            // an empty list makes cosmic-launcher skip the switcher entirely.
            let on_focused_output = !focused_outputs.is_empty()
                && (window.outputs.is_empty()
                    || window
                        .outputs
                        .iter()
                        .any(|output| focused_outputs.contains(output)));

            on_active_workspace && on_focused_output
        }
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
        WindowPlacement {
            workspaces: workspaces.to_vec(),
            outputs: outputs.to_vec(),
            focused: false,
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
}
